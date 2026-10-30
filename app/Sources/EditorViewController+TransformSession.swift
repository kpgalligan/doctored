import AppKit

extension EditorViewController {
    // MARK: - Free Transform

    /// What the current mouse drag does, captured at mouse-down together
    /// with the parameters it started from: every tick recomputes from that
    /// snapshot, so a drag never accumulates rounding error.
    enum TransformDrag {
        case move(start: LayerTransform, grab: CGPoint)
        case scale(handle: TransformHandle, start: LayerTransform)
        case rotate(start: LayerTransform, grab: CGPoint)
        case distort(corner: Int, start: LayerTransform)
    }

    /// Samplers offered for the commit-time resample, in popup order.
    /// Internal: the +ToolOptions descriptors list the titles.
    static let transformSamplers: [(title: String, value: DrResizeFilter)] = [
        ("Nearest", DR_FILTER_NEAREST),
        ("Bilinear", DR_FILTER_BILINEAR),
        ("Bicubic (Catmull-Rom)", DR_FILTER_CATMULL_ROM),
        ("Lanczos", DR_FILTER_LANCZOS3),
    ]

    /// Widest scale the numeric fields accept, mirroring
    /// LayerTransform.maxScaleMagnitude.
    static let maxScalePercent = Double(LayerTransform.maxScaleMagnitude) * 100

    /// How far from a corner (SCREEN points) the rotation ring reaches. The
    /// handles are tested first, so the ring is what is left of this radius
    /// outside the box.
    private static let transformRotateBand: CGFloat = 36

    private static func transformSamplerIndex(_ sampler: DrResizeFilter) -> Int {
        transformSamplers.firstIndex { $0.value == sampler } ?? 0
    }

    var isTransforming: Bool { transformSession != nil }

    /// Layer > Free Transform (⌘T). Opens the session on the active layer;
    /// Return commits it, Escape restores the untouched layer. With a
    /// selection active it transforms the active layer's SELECTED PIXELS
    /// instead, at either depth (EditorViewController+FloatingTransform.swift).
    @objc func freeTransform(_ sender: Any?) {
        // Never leave a text or shape session under the box, at every depth.
        guard canvas.commitTextSession(), commitShapeEditSession() else { return }
        if let document, document.canvasSelection != nil {
            beginFloatingTransform(document)
            return
        }
        if document?.doc?.precision != nil { beginPrecisionTransformSession(); return }
        guard !refuseOperationScope(.layerGeometry) else { return }
        guard transformSession == nil, let document = document else {
            NSSound.beep()
            return
        }
        // A transform is a POSITION edit, never a Pixels one: it resamples
        // the whole buffer including its alpha, so a frozen alpha channel
        // has no meaning there (doc_lock.rs). A transparency-locked layer
        // therefore still transforms; a position-locked one does not.
        guard let doc = document.doc else {
            NSSound.beep()
            return
        }
        // Over the EXPANDED set (MultiLayerEdit.swift), so a position-locked
        // LINKED partner or group descendant is named here rather than at the
        // commit.
        guard !refuseLockedEdit(
            layers: doc.movingSet(document.selectedLayerIndices), kind: DR_EDIT_POSITION)
        else { return }
        guard let session = makeTransformSession(doc, sampler: transformSampler) else {
            NSSound.beep()
            return
        }
        transformSession = session
        updateOptionsBar()
        refreshTransformPreview()
        updateTransformFields()
        updateStatus()
        view.window?.makeFirstResponder(canvas)
    }

    /// The parameters → controls half of the binding (the descriptors'
    /// setters are the other half): the bar re-reads every binding.
    private func updateTransformFields() {
        optionsBar.refreshValues()
    }

    // MARK: Free Transform gestures

    /// What a press at `point` grabs: a handle (⌘ on a corner pulls that
    /// corner alone — distort/perspective — otherwise scale), the ring just
    /// outside a corner (rotate), or the body (move). A press beyond all of
    /// them does nothing — clicking away must not silently commit.
    private func transformDrag(
        at point: CGPoint, session: TransformSession, modifiers: NSEvent.ModifierFlags
    ) -> TransformDrag? {
        let scale = canvas.magnification
        let slop = ImageCanvasView.transformHandleSize / scale
        for handle in TransformHandle.allCases {
            let world = session.transform.warpedHandlePoint(handle, of: session.sourceRect)
            if abs(point.x - world.x) <= slop, abs(point.y - world.y) <= slop {
                if modifiers.contains(.command), handle.isCorner,
                   let corner = handle.quadCorners.first {
                    return .distort(corner: corner, start: session.transform)
                }
                return .scale(handle: handle, start: session.transform)
            }
        }
        let corners = session.transform.warpedQuad(of: session.sourceRect)
        guard corners.count == 4 else { return nil }
        let box = NSBezierPath()
        box.move(to: corners[0])
        for corner in corners.dropFirst() {
            box.line(to: corner)
        }
        box.close()
        if box.contains(point) {
            return .move(start: session.transform, grab: point)
        }
        let band = Self.transformRotateBand / scale
        for corner in corners where hypot(point.x - corner.x, point.y - corner.y) <= band {
            return .rotate(start: session.transform, grab: point)
        }
        return nil
    }

    func transformMouseDown(_ point: CGPoint, _ modifiers: NSEvent.ModifierFlags) {
        guard let session = transformSession else { return }
        transformSession?.drag = transformDrag(at: point, session: session, modifiers: modifiers)
    }

    func transformMouseDragged(_ point: CGPoint, _ modifiers: NSEvent.ModifierFlags) {
        guard let session = transformSession, let drag = session.drag else { return }
        let proportional = modifiers.contains(.shift)
        let aboutPivot = modifiers.contains(.option)
        let updated: LayerTransform
        switch drag {
        case let .move(start, grab):
            updated = LayerTransform.moving(
                start, by: CGVector(dx: point.x - grab.x, dy: point.y - grab.y),
                constrained: proportional)
        case let .scale(handle, start):
            updated = LayerTransform.scaling(
                start, in: session.sourceRect, handle: handle, to: point,
                proportional: proportional, aboutPivot: aboutPivot)
        case let .rotate(start, grab):
            updated = LayerTransform.rotating(
                start, from: grab, to: point, snap: proportional)
        case let .distort(corner, start):
            updated = LayerTransform.distorting(
                start, corner: corner, to: point, in: session.sourceRect)
        }
        // ONE snap for all four drag kinds, restricted per kind in
        // DragSnapping.swift — a rotated, warped or ⇧-constrained handle
        // cannot reach an arbitrary point, so it does not snap at all.
        let snapped = snapTransform(
            updated, drag: drag, session: session, modifiers: modifiers)
        guard snapped.isFinite else { return }
        transformSession?.transform = snapped
        refreshTransformPreview()
        updateTransformFields()
    }

    /// Arrow-key nudge of the whole box (Shift: 10px).
    func transformNudge(_ dx: CGFloat, _ dy: CGFloat) {
        guard let session = transformSession else { return }
        transformSession?.transform = LayerTransform.moving(
            session.transform, by: CGVector(dx: dx, dy: dy), constrained: false)
        refreshTransformPreview()
        updateTransformFields()
    }

    // MARK: Free Transform options bar (descriptor bindings)

    // The session and its struct stay private; the +ToolOptions descriptors
    // read and write the parameters through these. Every setter re-renders
    // the preview and lets the bar re-read the whole set, so W tracks a
    // scale edit and vice versa. W = |scaleX| × base width, so typing W
    // sets scaleX = W / base width — preserving the current sign, so a
    // mirrored layer stays mirrored — through the same clamp the scale
    // setters use; bad input just snaps back to the current value.

    var transformDegrees: Double {
        get { transformSession?.transform.degrees ?? 0 }
        set {
            guard transformSession != nil else { return }
            transformSession?.transform.degrees = min(max(newValue, -360), 360)
            refreshTransformPreview()
        }
    }

    var transformScaleXPercent: Double {
        get { Double(transformSession?.transform.scaleX ?? 1) * 100 }
        set {
            guard transformSession != nil else { return }
            transformSession?.transform.scaleX = LayerTransform.clampScale(CGFloat(newValue / 100))
            refreshTransformPreview()
        }
    }

    var transformScaleYPercent: Double {
        get { Double(transformSession?.transform.scaleY ?? 1) * 100 }
        set {
            guard transformSession != nil else { return }
            transformSession?.transform.scaleY = LayerTransform.clampScale(CGFloat(newValue / 100))
            refreshTransformPreview()
        }
    }

    var transformWidthPixels: Double {
        get {
            guard let session = transformSession else { return 0 }
            return Double((abs(session.transform.scaleX) * session.sourceRect.width).rounded())
        }
        set {
            guard let session = transformSession, newValue > 0,
                  session.sourceRect.width > 0 else { return }
            let sign: CGFloat = session.transform.scaleX < 0 ? -1 : 1
            transformSession?.transform.scaleX = LayerTransform.clampScale(
                sign * CGFloat(newValue) / session.sourceRect.width)
            refreshTransformPreview()
        }
    }

    var transformHeightPixels: Double {
        get {
            guard let session = transformSession else { return 0 }
            return Double((abs(session.transform.scaleY) * session.sourceRect.height).rounded())
        }
        set {
            guard let session = transformSession, newValue > 0,
                  session.sourceRect.height > 0 else { return }
            let sign: CGFloat = session.transform.scaleY < 0 ? -1 : 1
            transformSession?.transform.scaleY = LayerTransform.clampScale(
                sign * CGFloat(newValue) / session.sourceRect.height)
            refreshTransformPreview()
        }
    }

    var transformSamplerListIndex: Int {
        get { Self.transformSamplerIndex(transformSession?.sampler ?? transformSampler) }
        set {
            let index = min(max(newValue, 0), Self.transformSamplers.count - 1)
            transformSampler = Self.transformSamplers[index].value
            transformSession?.sampler = transformSampler
            // Nearest previews without smoothing, so the box shows the hard
            // pixel edges the commit will produce.
            refreshTransformPreview()
        }
    }

    // MARK: Free Transform commit / cancel

    /// Runs the session's matrix through the core as ONE undo step named
    /// "Transform Layer" — composed into a described layer's description
    /// (no prompt) when the session is a plain affine, resampled otherwise.
    /// Returns false when the commit did NOT happen and the session must
    /// stay open: the core refused the matrix, or the user cancelled the
    /// rasterize prompt. A floating session commits "Transform
    /// Selected Pixels" through `commitFloatingTransform` instead, and a
    /// duplicate session "Transform Copy" through
    /// `commitDuplicateTransform`, first.
    ///
    /// A commit that published stores the session as the document's Transform
    /// Again memory (TransformAgain.swift); a true perspective stores its corners.
    @discardableResult
    func commitTransformSession() -> Bool {
        let document = self.document
        let session = transformSession
        let before = document?.doc
        let committed = commitTransformSessionEdit()
        if committed, let document, let session, let before, document.doc !== before {
            document.lastFreeTransform = RepeatableTransform(
                session: session.transform, sourceRect: session.sourceRect,
                sampler: session.sampler)
        }
        return committed
    }

    private func commitTransformSessionEdit() -> Bool {
        if let session = transformSession, session.duplicate != nil {
            return commitDuplicateTransform(session)
        }
        if let session = transformSession, session.floating != nil {
            return commitFloatingTransform(session)
        }
        guard let session = transformSession, let document = document, let doc = document.doc
        else {
            endTransformSession()
            return true
        }
        if session.precisionGesture != nil { return commitPrecisionTransformSession(session) }
        // Nothing actually moved (or the layer is gone): no edit, no undo
        // step, no dirty flag — just close the session.
        guard !session.transform.isIdentity, session.layers.allSatisfy({ $0 < doc.layerCount })
        else {
            endTransformSession()
            return true
        }
        let idx = session.primaryLayer
        // A distorted box commits through the perspective op with its warped
        // corners; a plain affine keeps the matrix path and its lossless
        // exact forms.
        let quad = session.transform.hasCornerOffsets
            ? session.transform.warpedQuad(of: session.sourceRect) : nil
        let matrix = session.transform.matrix
        let truePerspective = quad.map {
            LayerTransform.parallelogramAffine(of: session.sourceRect, onto: $0) == nil
        } ?? false
        // An affine on a described layer — plain, or a warped box that is
        // still a parallelogram — composes into the description and
        // re-renders (EditorViewController+DescribedTransform.swift): no
        // prompt. A true perspective quad, or a description that cannot
        // render right now (a missing font),
        // takes the rasterize prompt as before — resampling the
        // real pixels is what the prompt gates. Taken here rather than
        // through applyRasterizingEdit: Cancel has to keep the session open
        // instead of abandoning the whole gesture. Composing a matrix into
        // ONE description cannot stand for a SET, so only a single-entry
        // session takes the lossless compose path; a set resamples every
        // member (MultiLayerEdit.swift). The rules are `describedPrimaryGate`.
        switch describedPrimaryGate(
            document, source: doc, base: doc, primary: idx, single: session.isSingleLayer,
            matrix: matrix, quad: quad, truePerspective: truePerspective,
            sampler: session.sampler, record: transformRecord(session, quad: quad),
            name: "Transform Layer") {
        case .committed:
            endTransformSession()
            return true
        case .refused:
            return false
        case .resample:
            break
        }
        // Every other member's pre-check — the degenerate matrix, each
        // entry's own extent, the position locks and the remaining rasterize
        // prompts (MultiLayerEdit.swift). After the primary's prompt, so each
        // layer is asked exactly once.
        guard !refuseUntransformableSet(session, quad: quad) else { return false }
        let sampler = session.sampler
        session.corePreview?.cancelReleasingReservations()
        isCommittingTransform = true
        let committed = commitRasterTransformJob(
            document: document, source: doc, layers: session.layers, quad: quad,
            matrix: matrix, sampler: sampler, record: transformRecord(session, quad: quad),
            sourceRect: session.sourceRect)
        isCommittingTransform = false
        // The worker helper already reported a refusal (a degenerate scale, an
        // extent past its cap): keep the session up so the drag can be pulled
        // back, rather than losing it.
        guard committed else {
            refreshTransformPreview()
            return false
        }
        endTransformSession()
        return true
    }

    /// Escape, and the teardown every other exit funnels through. The
    /// document was never touched during the session, so cancelling is
    /// nothing more than dropping the preview.
    func endTransformSession() {
        guard transformSession != nil else { return }
        transformSession?.corePreview?.cancel()
        transformSession?.precisionGesture?.cancel()
        transformSession = nil
        canvas.transformPreview = nil
        // The box's smart guides go with the box (DragSnapping.swift): a
        // session torn down by Escape or a tool switch must not leave its
        // alignment lines drawn over the picture.
        pushSmartGuides([])
        updateOptionsBar()
        updateStatus()
        updateActiveLayerRect()
    }

    /// Closes an open session by COMMITTING it — the rule for every way of
    /// leaving one except Escape (tool switches, a new active layer, save,
    /// close). A commit the core or the user refuses leaves the session open,
    /// which is the safe direction, and answers false; true when no session
    /// remains.
    @discardableResult func commitPendingTransform() -> Bool {
        // Edit ▸ Puppet Warp's session commits on the same paths.
        guard commitPendingPuppetWarp() else { return false }
        // And Edit ▸ Perspective Warp's.
        guard commitPendingPerspectiveWarp() else { return false }
        guard transformSession != nil else { return true }
        return commitTransformSession()
    }
}
