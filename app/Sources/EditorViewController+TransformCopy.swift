import AppKit

// Opening a Transform Copy refuses an open session, float, the layer-geometry scope, at 16 bits a
// subtree set other than one entry, the originals' SUBTREE Position block.
// `commitTransformSession` routes it first. A described copy takes ⌘T's rules over
// `duplicate.source`: at 8 bits `describedPrimaryGate` (⌘T's and ⇧⌘T's primary block, over
// `source`/`base` — one copy under an affine composes through `commitDescribedTransform(…, source:,
// base:)`, a true perspective or a set prompts through `confirmRasterize(…, in:)`), and
// `refuseUntransformable(…, duplicate: true)` prompts the other described copies; at 16 bits a true
// perspective runs `confirmTruePerspectiveArtwork` over the copy and `.duplicateTransform` captures
// the copies' artwork (`PrecisionGeometryWork.duplicated`, `.transformSet`'s rule). Then 8 bits
// `commitRasterTransformJob(…, name:, base:)`, 16 bits `.duplicateTransform` through
// `finishOutcome`; ONE "Transform Copy" step recorded as `transform_layers` with `"duplicate":
// true` (and `rasterize_artwork` for a true perspective), the copies selected only after
// publication; Esc and identity publish nothing.

/// Layer ▸ Free Transform a Copy (⌥⌘T) with NO pixel selection: the
/// controller half — open, validate, commit. The model and the decisions live
/// in `DuplicateTransform.swift`.
///
/// Open refuses, in order: an open session (beep), a float document, the
/// layer-geometry scope, at 16 bits an originals' subtree set that is not
/// exactly one entry (beep, ⌘T's 16-bit gate) and the originals' SUBTREE
/// Position block. It then duplicates the originals into a
/// handle the document never holds and opens ⌘T's own session over the copies
/// (plus the typed gesture at 16 bits, captured on the document). Return takes
/// ⌘T's described rules over the copies and publishes ONE "Transform
/// Copy" step over the base, and only then selects the copies; Esc drops the
/// session and the document never changed.
extension EditorViewController {
    func beginDuplicateTransformSession() {
        guard transformSession == nil, let document, let doc = document.doc else {
            NSSound.beep()
            return
        }
        guard !refuseOperationScope(.layerGeometry) else { return }
        let originals = document.selectedLayerIndices
        let primary = document.activeLayerIndex
        let subtrees = MoveDuplicateLayers.subtrees(of: originals, in: doc)
        if doc.isHighDepth, subtrees.count != 1 {
            NSSound.beep()
            return
        }
        guard !refuseLockedEdit(layers: subtrees, kind: DR_EDIT_POSITION) else { return }
        let copy: DuplicateTransform
        do {
            guard let made = try DuplicateTransform(
                base: doc, originals: originals, primary: primary) else {
                document.presentError(DoctoredCoreError(message: "The copy could not be made."))
                return
            }
            copy = made
        } catch {
            document.presentError(error)
            return
        }
        guard var session = makeTransformSession(
            copy.source, sampler: transformSampler, set: copy.set) else {
            NSSound.beep()
            return
        }
        session.duplicate = copy
        if doc.isHighDepth {
            do {
                session.precisionGesture = try PrecisionGeometryGesture(document)
            } catch {
                document.presentError(error)
                return
            }
        }
        transformSession = session
        updateOptionsBar()
        refreshTransformPreview()
        optionsBar.refreshValues()
        updateStatus()
        view.window?.makeFirstResponder(canvas)
    }

    /// ⌘T's own no-selection validation: the layer-geometry scope and an
    /// active layer with pixels.
    func canBeginDuplicateTransform() -> Bool {
        guard supportsOperationScope(.layerGeometry) else { return false }
        let active = document?.activeLayerIndex ?? 0
        guard let info = document?.doc?.layerInfo(active) else { return false }
        return info.width > 0 && info.height > 0
    }

    /// Return: ONE "Transform Copy" step over the base, then the copies
    /// become the layer selection. False keeps the session open (a refusal,
    /// a stale base or a cancel).
    @discardableResult
    func commitDuplicateTransform(_ session: TransformSession) -> Bool {
        guard let copy = session.duplicate, let document else {
            endTransformSession()
            return true
        }
        if let gesture = session.precisionGesture {
            do { try gesture.capture.requireCurrent(document) } catch {
                document.presentError(error)
                return false
            }
        }
        // Nothing moved: no copy, no step — as ⌥⌘T over a selection.
        guard !session.transform.isIdentity else {
            endTransformSession()
            return true
        }
        let quad = session.transform.hasCornerOffsets
            ? session.transform.warpedQuad(of: session.sourceRect) : nil
        let matrix = session.transform.matrix
        let truePerspective = quad.map {
            LayerTransform.parallelogramAffine(of: session.sourceRect, onto: $0) == nil
        } ?? false
        let name = DuplicateTransform.actionName
        let record: [ActionStep] = .transformLayers(
            matrix: matrix, pivot: session.transform.pivot, sourceRect: session.sourceRect,
            corners: quad,
            sampler: Self.samplerAgentName(session.sampler), rasterizeArtwork: truePerspective,
            duplicate: true)
        // ⌘T's described rules over the copies: at 16 bits a true
        // perspective's artwork prompts (the `.transformSet` rule), at 8 bits
        // the primary's compose-or-prompt gate.
        if session.precisionGesture != nil {
            if truePerspective,
               !confirmTruePerspectiveArtwork(copy.source, layers: session.layers) {
                return false
            }
        } else {
            switch describedPrimaryGate(
                document, source: copy.source, base: copy.base, primary: session.primaryLayer,
                single: session.isSingleLayer, matrix: matrix, quad: quad,
                truePerspective: truePerspective, sampler: session.sampler, record: record,
                name: name) {
            case .committed:
                endTransformSession()
                setSelectedLayers(copy.selection)
                return true
            case .refused:
                return false
            case .resample:
                break
            }
        }
        guard !refuseUntransformableSet(session, quad: quad) else { return false }
        if let gesture = session.precisionGesture {
            isCommittingTransform = true
            defer { isCommittingTransform = false }
            do {
                let outcome = try gesture.finishOutcome(
                    precisionTransformRequest(session), name: name, record: record)
                guard outcome != .refused else {
                    refreshTransformPreview()
                    return false
                }
            } catch is JobCancelled {
                // The gesture re-armed itself: keep the session, ask again.
                SessionCommitRefusal.note(JobCancelled.message)
                refreshTransformPreview()
                return false
            } catch {
                document.presentError(error)
                return false
            }
        } else {
            session.corePreview?.cancelReleasingReservations()
            isCommittingTransform = true
            let committed = commitRasterTransformJob(
                document: document, source: copy.source, layers: session.layers, quad: quad,
                matrix: matrix, sampler: session.sampler, record: record,
                name: name, base: copy.base, sourceRect: session.sourceRect)
            isCommittingTransform = false
            guard committed else {
                refreshTransformPreview()
                return false
            }
        }
        endTransformSession()
        // After publication, never before.
        if document.doc !== copy.base { setSelectedLayers(copy.selection) }
        return true
    }
}
