import AppKit

// When a selection is active, `freeTransform` takes `beginFloatingTransform` first (before the
// typed branch and the layer-geometry scope). It runs the Pixels-lock owner alert,
// `TransformSelectedPixels.refusal` (`MoveSelectedPixels.refusal` with `verb`/`gerund`), the
// empty box and `confirmRasterize`, and THEN captures base, selection and coverage (the prompt
// services MCP). The session is an ordinary `TransformSession` whose trailing `floating:
// FloatingSelection?` routes `refreshTransformPreview` (8-bit
// `TransformMaskPreview.request(key:identity:scratch:compute:)` over the meta-dropped source, its
// scratch the core plan of EACH request; 16-bit `refreshPrecisionTransformPreview(_:request:
// .transformSelected)`) and `commitTransformSession` (`commitFloatingTransform`).
//
// Layer ▸ Free Transform a Copy (⌥⌘T, `freeTransformCopy`, validated by
// `validateFreeTransformCopyItem()` — a selection, or with none ⌘T's own no-selection validation,
// where it copies the layers) is the same session with `FloatingSelection.duplicate`: both
// `command`s and `record` carry `"duplicate": true`, the core leaves no hole, the commit is ONE
// "Transform Copy" step (`duplicateActionName`) and the status line reads "Free Transform · copy of
// selected pixels"; MCP `duplicate` is its twin. With a MASK or ALPHA CHANNEL as
// `planeEditTarget`, `beginFloatingTransform` takes `beginPlaneTransform` BEFORE the Pixels-lock
// alert: `refusal(planes: true)`, `planeBox` (the selection's bounds, no float-box wait or
// rasterize prompt), the background swatch through `ActionArgs.hex` → `planeBackground` (coverage
// gray ×257), and `FloatingSelection.target`/`background` in every `command` and `record`; the
// preview shows the transformed plane (`planeDisplay`, the `display` hook of `TransformMaskPreview`
// / `PrecisionGeometryGesture`) only when the canvas base IS that plane. ⇧⌘T's
// `transformAgainPlane` and MCP `transform_again`'s plane branch take the same path; both MCP tools
// take `target`/`background` and answer `mask_bounds` or `channel`; a float document admits a plane
// target and the layer's own pixels. A RED, GREEN, BLUE or ALPHA target takes
// `beginPlaneTransform` too, after the layer path's lock alert (`planeLockKind`) and with its
// rasterize prompt (the 8-bit source drops the description); `planeBackground` answers the swatch.

/// Free Transform of the selected pixels: ⌘T with a selection active.
/// The model, refusals and request live in `FloatingSelection.swift`; this
/// file is the controller half — open, preview and commit.
///
/// The session is an ordinary `TransformSession` (the handles, the options
/// bar, the drags and Escape are all ⌘T's own) carrying a `floating`
/// capture. Its box is the FLOAT's opaque bounds, found by the core
/// under one short cancellable wait after the rasterize prompt. The
/// preview is a latest-wins background core edit of the WHOLE document (8
/// bits: `TransformMaskPreview`; 16 bits: the typed `PrecisionGeometryGesture`
/// with `.transformSelected`; the 8-bit preview plans each request), so a
/// work refusal lands in the status line before Return. The commit runs the
/// pixel edit and then the transformed selection in ONE background job
/// and publishes both as ONE "Transform Selected Pixels" step — after a check
/// that no edit landed since the capture (the wait services MCP).
///
/// With a mask or alpha channel as the edit target the session floats
/// that plane (`beginPlaneTransform`): no Pixels-lock alert, rasterize prompt
/// or float-box wait — the box is the selection's bounds — and the preview
/// shows the transformed plane when the canvas base is that plane.
extension EditorViewController {
    /// Layer ▸ Free Transform a Copy (⌥⌘T): ⌘T over the selected pixels
    /// with `duplicate` — the copy transforms and the original stays. With no
    /// selection it duplicates the target layers and transforms the copies
    /// (EditorViewController+TransformCopy.swift).
    @objc func freeTransformCopy(_ sender: Any?) {
        // Never leave a text or shape session under the box, as ⌘T does.
        guard canvas.commitTextSession(), commitShapeEditSession() else { return }
        if let document, document.canvasSelection != nil {
            beginFloatingTransform(document, duplicate: true)
            return
        }
        beginDuplicateTransformSession()
    }

    func validateFreeTransformCopyItem() -> Bool {
        document?.canvasSelection != nil || canBeginDuplicateTransform()
    }

    func beginFloatingTransform(_ document: ImageDocument, duplicate: Bool = false) {
        guard transformSession == nil else {
            NSSound.beep()
            return
        }
        // Never leave a text session hanging underneath the box.
        guard canvas.commitTextSession() else { return }
        guard let doc = document.doc, let selection = document.canvasSelection else { return }
        let layer = document.activeLayerIndex
        // A mask or channel target floats that plane, before the
        // layer's Pixels-lock alert and box.
        let planeTarget = document.planeEditTarget
        if planeTarget != .layer {
            beginPlaneTransform(document, target: planeTarget, layer: layer,
                                selection: selection, duplicate: duplicate)
            return
        }
        if refuseLockedEdit(layer: layer, kind: DR_EDIT_PIXELS) { return }
        if let reason = TransformSelectedPixels.refusal(
            document, layer: layer, target: document.planeEditTarget) {
            document.presentError(DoctoredCoreError(message: reason))
            return
        }
        guard TransformSelectedPixels.box(doc, layer: layer, selection: selection) != nil else {
            document.presentError(DoctoredCoreError(message: TransformSelectedPixels.emptyRefusal))
            return
        }
        let described = document.layerDescribesSource(layer)
        if described, !document.confirmRasterize(layer: layer) { return }
        // AFTER the prompt, whose modal loop services MCP: everything below
        // derives from this capture, and the commit refuses if it went stale.
        guard self.document === document, transformSession == nil, let base = document.doc,
              let captured = document.canvasSelection,
              TransformSelectedPixels.box(base, layer: layer, selection: captured) != nil
        else {
            document.presentError(DoctoredCoreError(message: TransformSelectedPixels.staleRefusal))
            return
        }
        let history = document.selectionHistory
        let coverage = captured.maskSamples16()
        // The box is the float's opaque bounds. The wait services MCP,
        // so the base, selection and layer it measured must still be the
        // document's after it, or the box would frame another float.
        let box: CGRect
        do {
            box = try TransformSelectedPixels.floatBox(
                base, layer: layer, coverage: coverage, title: "Free Transform")
        } catch {
            document.presentError(error)
            return
        }
        guard self.document === document, transformSession == nil, document.doc === base,
              document.selectionHistory == history, document.activeLayerIndex == layer
        else {
            document.presentError(DoctoredCoreError(message: TransformSelectedPixels.staleRefusal))
            return
        }
        var source = base
        var budget: (limit: UInt64, other: UInt64)?
        var corePreview: TransformMaskPreview?
        var gesture: PrecisionGeometryGesture?
        do {
            if base.isHighDepth {
                gesture = try PrecisionGeometryGesture(document, indexedBackground: backgroundColor)
            } else {
                if described {
                    guard let dropped = base.withLayerMeta(layer, nil) else {
                        throw DoctoredCoreError(
                            message: "The layer's description could not be dropped.")
                    }
                    source = dropped
                }
                budget = try base.legacyPrecisionWorkContext()
                corePreview = TransformMaskPreview(
                    source: source, revision: document.revision, image: document.previewImage)
            }
        } catch {
            document.presentError(error)
            return
        }
        let floating = FloatingSelection(
            layer: layer, base: base, source: source, selection: history, coverage: coverage,
            described: described, budget: budget, duplicate: duplicate)
        transformSession = TransformSession(
            layers: [layer], sourceRect: box, layerImage: nil, maskImage: nil,
            below: nil, above: nil, opacity: 1, corePreview: corePreview,
            precisionGesture: gesture,
            transform: LayerTransform(pivot: CGPoint(x: box.midX, y: box.midY)),
            sampler: transformSampler, drag: nil, floating: floating)
        updateOptionsBar()
        refreshTransformPreview()
        optionsBar.refreshValues()
        updateStatus()
        view.window?.makeFirstResponder(canvas)
    }

    /// ⌘T / ⌥⌘T over the selected area of the targeted mask or alpha
    /// channel. The box is the selection's bounds; the vacated area takes the
    /// background swatch as coverage gray. A colour plane takes the
    /// layer path's lock alert and rasterize prompt, and the swatch's 30/59/11
    /// luma rounded to a code as its vacated value.
    private func beginPlaneTransform(_ document: ImageDocument, target: PaintTarget, layer: Int,
                                     selection: CanvasSelection, duplicate: Bool) {
        if let kind = MoveSelectedPixels.planeLockKind(target),
           refuseLockedEdit(layer: layer, kind: kind) { return }
        if let reason = TransformSelectedPixels.refusal(
            document, layer: layer, target: target, planes: true) {
            document.presentError(DoctoredCoreError(message: reason))
            return
        }
        guard document.doc != nil, TransformSelectedPixels.planeBox(selection) != nil else {
            document.presentError(DoctoredCoreError(message: TransformSelectedPixels.emptyRefusal))
            return
        }
        let described = target.isPlane && document.layerDescribesSource(layer)
        if described, !document.confirmRasterize(layer: layer) { return }
        // AFTER the prompt, whose modal loop services MCP.
        guard self.document === document, transformSession == nil, let base = document.doc,
              let captured = document.canvasSelection,
              let box = TransformSelectedPixels.planeBox(captured) else {
            document.presentError(DoctoredCoreError(message: TransformSelectedPixels.staleRefusal))
            return
        }
        let history = document.selectionHistory
        let coverage = captured.maskSamples16()
        let hex = ActionArgs.hex(backgroundColor)
        let display: ((RasterDocument) -> CGImage?)? =
            canvas.channelDisplay?.baseKind.coverageTarget == target
            ? TransformSelectedPixels.planeDisplay(target: target, layer: layer) : nil
        let initial = display == nil ? nil : canvas.channelDisplay?.base
        var budget: (limit: UInt64, other: UInt64)?
        var corePreview: TransformMaskPreview?
        var gesture: PrecisionGeometryGesture?
        let background: PlaneBackground
        var source = base
        do {
            background = try TransformSelectedPixels.planeBackground(
                hex: hex, target: target, in: document)
            if base.isHighDepth {
                gesture = try PrecisionGeometryGesture(
                    document, display: display, initialImage: initial,
                    indexedBackground: backgroundColor)
            } else {
                if described {
                    guard let dropped = base.withLayerMeta(layer, nil) else {
                        throw DoctoredCoreError(
                            message: "The layer's description could not be dropped.")
                    }
                    source = dropped
                }
                budget = try base.legacyPrecisionWorkContext()
                corePreview = TransformMaskPreview(
                    source: source, revision: document.revision,
                    image: initial ?? document.previewImage, display: display)
            }
        } catch {
            document.presentError(error)
            return
        }
        let floating = FloatingSelection(
            layer: layer, base: base, source: source, selection: history, coverage: coverage,
            described: described, budget: budget, duplicate: duplicate, target: target,
            background: background)
        transformSession = TransformSession(
            layers: [layer], sourceRect: box, layerImage: nil, maskImage: nil,
            below: nil, above: nil, opacity: 1, corePreview: corePreview,
            precisionGesture: gesture,
            transform: LayerTransform(pivot: CGPoint(x: box.midX, y: box.midY)),
            sampler: transformSampler, drag: nil, floating: floating)
        updateOptionsBar()
        refreshTransformPreview()
        optionsBar.refreshValues()
        updateStatus()
        view.window?.makeFirstResponder(canvas)
    }

    /// Asks for the transformed document from the captured source; the box
    /// redraws at once, the pixels follow as each preview lands.
    func refreshFloatingTransformPreview(_ session: TransformSession) {
        guard let floating = session.floating else { return }
        let transform = session.transform
        let matrix = transform.matrix
        let quad = transform.hasCornerOffsets ? transform.warpedQuad(of: session.sourceRect) : nil
        let sampler = session.sampler
        let source = floating.source
        let coverage = floating.coverage
        let budget = floating.budget
        guard let command = try? TransformSelectedPixels.command(
            layer: floating.layer, box: session.sourceRect, matrix: matrix, quad: quad,
            sampler: sampler, bounds: floating.selection.selection?.bounds,
            duplicate: floating.duplicate, target: floating.target,
            background: floating.background) else { return }
        let request = command.request
        if session.precisionGesture != nil {
            refreshPrecisionTransformPreview(
                session, request: .transformSelected(command: command, coverage: coverage))
            return
        }
        guard let preview = session.corePreview else { return }
        // Not `doc === preview.source`: a described source is the
        // meta-dropped copy, never the document's own handle.
        preview.onUpdate = { [weak self, weak preview] in
            guard let self, let preview, let current = self.transformSession,
                  current.corePreview === preview,
                  self.document?.revision == preview.revision else { return }
            self.displayMaskedTransformPreview(current, preview: preview)
            self.updateStatus()
        }
        preview.request(key: TransformSelectedPixels.previewKey(
                            matrix: matrix, quad: quad, sampler: sampler),
                        identity: transform.isIdentity, scratch: {
            // The landing term follows this request's geometry.
            try PrecisionDocumentDescriptor.integer(
                source.selectedPixelsPlan(request: request), "scratch_bytes")
        }) { control in
            guard let transformed = try source.editingSelectedPixels(
                request: request, coverage: coverage, budget: budget, control: control)
            else { throw DoctoredCoreError(message: "Could not preview the transform.") }
            return transformed
        }
        displayMaskedTransformPreview(session, preview: preview)
    }

    /// Return: ONE "Transform Selected Pixels" step with the transformed
    /// selection. False keeps the session open (a refusal, or a stale base).
    @discardableResult
    func commitFloatingTransform(_ session: TransformSession) -> Bool {
        guard let floating = session.floating, let document else {
            endTransformSession()
            return true
        }
        guard !session.transform.isIdentity else {
            endTransformSession()
            return true
        }
        let transform = session.transform
        let box = session.sourceRect
        let matrix = transform.matrix
        let quad = transform.hasCornerOffsets ? transform.warpedQuad(of: box) : nil
        let sampler = session.sampler
        let name = floating.duplicate
            ? TransformSelectedPixels.duplicateActionName : TransformSelectedPixels.actionName
        let record = TransformSelectedPixels.record(
            floating.base, layer: floating.layer, transform: transform, quad: quad,
            sampler: sampler, exact: session.precisionGesture != nil,
            duplicate: floating.duplicate,
            target: floating.target == .layer ? nil : floating.target.agentName(in: floating.base),
            background: floating.background?.hex)
        do {
            session.corePreview?.cancelReleasingReservations()
            isCommittingTransform = true
            defer { isCommittingTransform = false }
            // A refused publication keeps the session and leaves no undo step;
            // a geometry that changed nothing ends it.
            let outcome = try publishSelectedPixelsTransform(
                document, floating, box: box, matrix: matrix, quad: quad, sampler: sampler,
                name: name, record: record, gesture: session.precisionGesture)
            guard outcome != .refused else {
                refreshTransformPreview()
                return false
            }
        } catch is JobCancelled {
            SessionCommitRefusal.note(JobCancelled.message)
            refreshTransformPreview()
            return false
        } catch {
            document.presentError(error)
            refreshTransformPreview()
            return false
        }
        endTransformSession()
        return true
    }

    /// The selected-pixels publication ⌘T's commit and Transform Again
    /// share: the stale check, the shared `transform_selected` command and the
    /// transformed selection, then the 16-bit `gesture`'s `finishOutcome` or
    /// the 8-bit prepared edit. `.unchanged` when no pixel changed.
    func publishSelectedPixelsTransform(
        _ document: ImageDocument, _ floating: FloatingSelection, box: CGRect,
        matrix: CGAffineTransform, quad: [CGPoint]?, sampler: DrResizeFilter, name: String,
        record: [ActionStep], gesture: PrecisionGeometryGesture?
    ) throws -> PublishOutcome {
        // Early, to save the planning: the wait below re-checks.
        guard document.doc === floating.base,
              document.selectionHistory == floating.selection else {
            throw DoctoredCoreError(message: TransformSelectedPixels.staleRefusal)
        }
        let command = try TransformSelectedPixels.command(
            layer: floating.layer, box: box, matrix: matrix, quad: quad, sampler: sampler,
            bounds: floating.selection.selection?.bounds, duplicate: floating.duplicate,
            target: floating.target, background: floating.background)
        // Plans the pixel edit first; the selection is computed in the
        // pixel job.
        let transformed = try TransformedSelection(
            source: floating.base, command: command, captured: floating.selection,
            coverage: floating.coverage, box: box, matrix: matrix, quad: quad,
            sampler: sampler)
        if let gesture {
            // `finish` refuses at entry unless the capture made at open is
            // still current.
            return try gesture.finishOutcome(
                .transformSelected(command: command, coverage: floating.coverage),
                name: name, record: record, transformedSelection: transformed)
        }
        guard let prepared = try MoveSelectedPixels.prepared(
            document, source: floating.source, request: command.request,
            coverage: floating.coverage, title: name, base: floating.base,
            selection: floating.selection, stale: TransformSelectedPixels.staleRefusal,
            transformedSelection: transformed)
        else { return .unchanged }
        document.preparedEdit = prepared.edit
        guard document.admitsPublication(prepared.edit.document, record: record) else {
            return .refused
        }
        return document.publishPrepared(
            prepared.edit, name: name, record: record, resultingSelection: prepared.selection)
    }
}
