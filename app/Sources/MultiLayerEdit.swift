import AppKit

/// Editing SEVERAL layers as one gesture: the Free Transform session, the
/// Move drag — everything whose unit of work is a SET of entries rather than
/// the active layer alone.
///
/// It carries the Free Transform session outright, moved here from the frozen
/// `EditorViewController.swift` (which is why that file ends this phase
/// net-negative): the session is the one piece of state that has to know
/// about several layers at once, and a `layers: [Int]` field is the whole
/// difference between transforming one layer and transforming a selection.
///
/// **Three rules everything here obeys.**
///
/// 1. **One core call per tick, never a host loop.** `moveLayers` and
///    `transformLayers` take the whole set, so subtrees and link groups are
///    expanded once, in the core, and a position lock refuses the whole call
///    instead of half of it. A loop would also renumber under itself the
///    moment a structural op is involved.
/// 2. **The set is expanded by SUBTREE and then by LINK GROUP.**
///    `RasterDocument.movingSet` mirrors `doc_group_query::expanded_set` for
///    the preview and for the refusal message; the core still decides the
///    edit, so the two can differ only in what the user is *told*, never in
///    what happens.
/// 3. **A transform is a POSITION edit, never a Pixels one** (`doc_lock.rs`):
///    it resamples the whole buffer including its alpha, so a frozen alpha
///    channel has no meaning there. A transparency-locked layer therefore
///    still transforms; a position-locked one does not.
extension EditorViewController {
    /// A modal free-transform session. The document is NOT touched while it
    /// runs: the canvas draws the layers' cached pixels through the composed
    /// matrix, and the core resamples exactly once, at commit. The
    /// parameters (see LayerTransform) are the source of truth — handle
    /// drags and the options-bar numerics are two ways of writing them, and
    /// the matrix is always composed, never decomposed.
    struct TransformSession {
        /// The entries being transformed, ascending. The session outlives
        /// changes to the document's selection, so it carries its own list;
        /// one matrix is applied to every one of them, each deriving its own
        /// destination extent.
        ///
        /// It is the EXPANDED set — subtrees and link groups included — so
        /// what the box promises and what the commit moves are the same
        /// entries.
        let layers: [Int]
        /// The union rect of `layers` in canvas space when the session
        /// opened — what the box, the handles and the W/H fields describe.
        let sourceRect: CGRect
        let layerImage: CGImage?
        let maskImage: CGImage?
        let below: CGImage?
        let above: CGImage?
        let opacity: CGFloat
        let corePreview: TransformMaskPreview?
        var precisionGesture: PrecisionGeometryGesture? = nil
        var transform: LayerTransform
        var sampler: DrResizeFilter
        var drag: TransformDrag?
        /// Set when ⌘T opened over a selection (FloatingSelection.swift):
        /// the box transforms the active layer's SELECTED PIXELS.
        var floating: FloatingSelection? = nil
        /// Set when ⌥⌘T opened with no selection (DuplicateTransform.swift):
        /// `layers`, `sourceRect` and the plates are then indices and pixels
        /// of `duplicate.source`, never of the document.
        var duplicate: DuplicateTransform? = nil

        /// The entry the single-layer paths still need: the rasterize
        /// prompt, the described-layer compose, the status line. Lowest
        /// index of the set, which for a one-layer session is simply the
        /// layer it opened on.
        var primaryLayer: Int { layers.first ?? 0 }

        /// True when the session is exactly one entry, which is when the
        /// single-layer commit paths (compose into a description, the
        /// perspective quad) mean anything at all.
        var isSingleLayer: Bool { layers.count == 1 }
    }

    // MARK: - The Move drag

    /// A Move press: Auto-Select picks the entry under the cursor when the
    /// option is on (EditorViewController+Groups.swift), then the drag
    /// begins on whatever the selection is. The point is the canvas-clamped
    /// press position — a press outside the canvas cannot hit a layer
    /// anyway, so clamping only makes the edge pixel the answer.
    ///
    /// `moveAppliedDelta` — the one stored property the frozen file keeps for
    /// this gesture — holds the delta ALREADY APPLIED, starting at zero, not
    /// a layer's start offset. Two reasons, both real:
    ///
    /// - a GROUP has no offset of its own (`dr_doc_layer_offset_x` answers
    ///   with its CONTENT bounds' origin, and (0, 0) when nothing in it is
    ///   opaque), so an offset-relative step would re-apply the whole drag
    ///   delta on every tick and the group would run away under the cursor;
    /// - the tracked delta is exact whatever the core did with the entries —
    ///   a refused tick simply leaves the remaining delta to the next one.
    ///
    /// With a selection active the drag moves the SELECTED PIXELS of the
    /// active layer instead (EditorViewController+MoveSelected.swift),
    /// decided BEFORE the Position lock, which that move does not answer to.
    /// ⌥ with no selection duplicates the selected layers and drags the
    /// copies (EditorViewController+MoveDuplicate.swift).
    func moveDidBegin(at point: CGPoint, modifiers: NSEvent.ModifierFlags) {
        guard let document = document, document.doc != nil else { return }
        // A selected-pixels drag whose mouse-up never arrived (a modal alert
        // swallowed it) commits now, as its release would have.
        if moveSelectedSession != nil { finishMoveSelected() }
        if moveDuplicate != nil { finishMoveDuplicate() }
        // The previous drag's leftovers go first: a smart guide belongs to
        // the gesture that earned it, and `movePressBox` must never be read
        // by a drag that did not write it.
        moveDidEnd()
        autoSelectLayer(at: point, modifiers: modifiers)
        if document.canvasSelection != nil {
            // ⌥ latched at the press: the drag duplicates.
            beginMoveSelected(document, duplicate: modifiers.contains(.option))
            return
        }
        // A POSITION lock refuses the drag before it opens a live edit, and
        // the alert names the layer that stopped it. Checked over the
        // EXPANDED set, because that is what would move — for a duplicate,
        // the originals' subtrees, whose copies carry their locks.
        let duplicate = modifiers.contains(.option)
        let moving = document.doc.map {
            duplicate ? MoveDuplicateLayers.subtrees(of: document.selectedLayerIndices, in: $0)
                : $0.movingSet(document.selectedLayerIndices)
        } ?? []
        if refuseLockedEdit(layers: moving, kind: DR_EDIT_POSITION) { return }
        // What the drag SNAPS: a delta cannot be snapped — only a box can —
        // and nothing during the gesture otherwise knows where the moving set
        // is, because the canvas reports an offset. `transformSourceRect` is
        // the group-aware union the Free Transform box opens with, so a Move
        // and a ⌘T over the same selection snap the same rectangle.
        movePressBox = document.doc.flatMap { transformSourceRect($0, layers: moving) }
        if duplicate { beginMoveDuplicate(document) }
        moveAppliedDelta = (0, 0)
        if document.doc?.isHighDepth == true {
            beginPrecisionMoveGesture(document)
            return
        }
        document.beginLiveEdit()
    }

    /// The Move drag's teardown: the press-time box and the smart guides are
    /// both the gesture's, and neither survives it.
    ///
    /// Called from BOTH ends of the gesture: from `canvas.onMoveEnd` when
    /// the mouse comes up, which is what takes the lines off the screen at
    /// the moment the drag finishes, and again at the start of
    /// `moveDidBegin`, so a gesture can never inherit the previous one's
    /// press box or its smart guides — an interrupted drag (a session that
    /// tore the gesture down, a document swapped underneath it) has no
    /// mouse-up to run the first call.
    func moveDidEnd() {
        movePressBox = nil
        pushSmartGuides([])
    }

    /// One drag tick, as ONE `moveLayers` call so linked entries and group
    /// subtrees follow and a position lock refuses the whole set.
    ///
    /// The canvas reports the TOTAL delta from the press, so the step is that
    /// total minus what has already been applied.
    ///
    /// `modifiers` carries ⌃, which suspends snapping for the tick — read
    /// per tick, never latched, so it can be pressed and released mid-drag.
    ///
    /// The snap goes on the TOTAL delta, ABOVE the step subtraction, so the
    /// existing bookkeeping absorbs the correction for free — exactly as it
    /// already absorbs a core refusal. `moveAppliedDelta` therefore tracks
    /// the SNAPPED total: the next tick's step is measured from where the
    /// layers actually are, which is what makes a snapped set stick to its
    /// line while the pointer wanders inside the pull radius.
    func moveDidUpdate(_ dx: Int, _ dy: Int, _ modifiers: NSEvent.ModifierFlags) {
        // A drag over a selection moves the selected pixels, snapped
        // the same way (its press box is the selection on the layer).
        if moveSelectedSession != nil {
            let (totalX, totalY) = snapMoveDelta(dx, dy, modifiers)
            updateMoveSelected(dx: totalX, dy: totalY)
            return
        }
        let (totalX, totalY) = snapMoveDelta(dx, dy, modifiers)
        if moveDuplicate != nil {
            updateMoveDuplicate(dx: totalX, dy: totalY)
            return
        }
        guard let document = document, let doc = document.doc,
              let applied = moveAppliedDelta
        else { return }
        if doc.isHighDepth {
            moveAppliedDelta = (totalX, totalY)
            updatePrecisionMoveGesture(dx: totalX, dy: totalY)
            return
        }
        let stepX = totalX - applied.x
        let stepY = totalY - applied.y
        guard stepX != 0 || stepY != 0 else { return }
        // Byte-only (the typed drag returned above), so nothing is thrown
        // here; `try?` only satisfies the throwing facade. A tick does not
        // re-render a pattern fill; `moveDidFinish` refits once.
        guard let updated = try? doc.moveLayers(
            document.selectedLayerIndices, dx: stepX, dy: stepY, refit: false)
        else { return }
        moveAppliedDelta = (totalX, totalY)
        document.updateLiveEdit(updated)
    }

    /// An arrow-key nudge of the whole selection: the store's nudge step, ten
    /// times it with Shift (the canvas decides the step). One `moveLayers`
    /// call and one undo step per press, the same op the drag ticks use, so a
    /// nudged group and a nudged link group behave exactly like a dragged
    /// one.
    ///
    /// A nudge deliberately does NOT snap: an arrow key is an explicit
    /// N-pixel request, and a snap would swallow it whole whenever a guide
    /// sat inside the pull radius — the one keystroke whose whole purpose is
    /// to move by exactly what it says.
    ///
    /// With a selection active a nudge moves the selected pixels,
    /// decided before the Position lock, which that move does not answer to.
    /// ⌥ (`duplicate`) duplicates: the selected pixels with a
    /// selection, the selected layers without one.
    func moveNudge(_ dx: Int, _ dy: Int, duplicate: Bool) {
        guard let document = document, let doc = document.doc else { return }
        if document.canvasSelection != nil {
            moveSelectedNudge(dx, dy, duplicate: duplicate)
            return
        }
        if duplicate {
            moveDuplicateNudge(dx, dy)
            return
        }
        let indices = document.selectedLayerIndices
        guard !refuseLockedEdit(layers: doc.movingSet(indices), kind: DR_EDIT_POSITION)
        else { return }
        if doc.isHighDepth {
            precisionMoveNudge(indices: indices, dx: dx, dy: dy)
            return
        }
        document.applyEdit("Move Layer", record: Self.moveRecord(doc, indices, dx: dx, dy: dy)) {
            try $0.moveLayers(indices, dx: dx, dy: dy)
        }
    }

    /// What a Move — a nudge or a drag — records: `move_layers` with the
    /// gesture's own DELTA, at either size.
    ///
    /// **Both halves are relative now (Phase 12B).** A one-layer Move used to
    /// record an absolute `set_layer_properties` offset and say so in a note,
    /// because `move_layers` had no single-layer spelling: replaying it
    /// snapped the layer to a fixed canvas position instead of nudging it by
    /// the amount the user dragged. It now records the same relative step the
    /// set form does, naming the one layer by reference.
    ///
    /// **The set is the EXPANDED one, not the panel selection.** The gesture
    /// moves `movingSet(indices)` — the subtree and the link group with it —
    /// so a one-ENTRY panel selection that is LINKED to another layer is a
    /// multi-entry move and records the `$layers` form, which is what makes a
    /// replay move the watermark and the caption together.
    ///
    /// A `duplicate` records `"duplicate": true` and sizes its set by
    /// the originals' SUBTREES: the copies are unlinked, so no link partner
    /// is copied or moved.
    static func moveRecord(
        _ doc: RasterDocument, _ indices: [Int], dx: Int, dy: Int, duplicate: Bool = false
    ) -> [ActionStep] {
        let moving = duplicate
            ? MoveDuplicateLayers.subtrees(of: indices, in: doc) : doc.movingSet(indices)
        if moving.count > 1 {
            return .moveLayersStep(layers: [], dx: dx, dy: dy, duplicate: duplicate)
        }
        guard moving.count == 1, let idx = moving.first, let ref = doc.layerRef(idx)
        else { return .unrecorded("Move Layers") }
        return .moveLayersStep(layers: [ref], dx: dx, dy: dy, duplicate: duplicate)
    }

    /// What the status line says while a Free Transform is open. A SET names
    /// how many entries the one matrix will move, because the box alone
    /// cannot say: a non-contiguous selection still draws ONE rectangle,
    /// around layers that need not be neighbours, and the count is the only
    /// on-screen statement of what is inside it.
    var transformStatusText: String {
        guard let session = transformSession else { return "Free Transform" }
        let name = session.floating?.statusName
            ?? (session.duplicate != nil
                ? (session.isSingleLayer ? "Free Transform · copy"
                    : "Free Transform · copy of \(session.layers.count) layers")
            : session.isSingleLayer ? "Free Transform"
            : "Free Transform · \(session.layers.count) layers")
        if let gesture = session.precisionGesture { return name + " · " + gesture.status }
        return session.corePreview.map { name + " · " + $0.status } ?? name
    }

    // MARK: - Opening a Free Transform session

    /// Builds the session ⌘T opens: the whole selection, expanded by subtree
    /// and link group, with one box around it. nil when there is nothing to
    /// transform (no entry with pixels), which the caller answers with a
    /// beep.
    ///
    /// **The single-entry case is deliberately byte-for-byte what it has
    /// always been** — the box is the layer's PIXEL rect, the preview its own
    /// pixels drawn at its own opacity through its own mask, the pivot its
    /// description's centre when it has one. That rect is what the W/H fields
    /// describe and what the extent pre-check measures, and a layer's buffer
    /// is the honest answer for both.
    ///
    /// A SET has no single buffer, so its box is the union of the entries'
    /// CONTENT bounds (`layerBounds`) — the opaque box, which is what a
    /// person means by "these three layers" — and its preview is the set
    /// composited once and cut out at that box, so masks, opacities and
    /// blend modes inside the set are already baked and the preview draws at
    /// opacity 1.
    ///
    /// `set` nil is `doc.movingSet` of the layer selection; Free Transform a
    /// Copy passes the copies' subtrees in the duplicated `doc`.
    func makeTransformSession(_ doc: RasterDocument, sampler: DrResizeFilter,
                              set: [Int]? = nil) -> TransformSession?
    {
        guard let document = document else { return nil }
        let set = set ?? doc.movingSet(document.selectedLayerIndices)
        guard let primary = set.first else { return nil }
        let single = set.count == 1 && !doc.layerIsGroup(primary)
            && !doc.layerIsAdjustment(primary)
        let pixelRect: CGRect? = doc.layerInfo(primary).map {
            CGRect(
                x: CGFloat($0.offsetX), y: CGFloat($0.offsetY),
                width: CGFloat($0.width), height: CGFloat($0.height))
        }
        // A mask's independent placement, feather/density, and group gates
        // must be evaluated after geometry. Baking the old coverage into a
        // plate and moving that plate cannot represent an unlinked mask.
        let needsCorePreview = doc.isHighDepth || set.contains(where: doc.hasRetainedSource) || (0..<doc.layerCount).contains {
            doc.layerHasMask($0) && doc.layerMaskEnabled($0)
        }
        let multiRect = single ? nil : (needsCorePreview ? transformPixelSourceRect(doc, layers: set)
            : transformSourceRect(doc, layers: set))
        guard let rect = single ? pixelRect : multiRect, rect.width >= 1, rect.height >= 1 else { return nil }
        let corePreview = needsCorePreview && !doc.isHighDepth
            ? TransformMaskPreview(source: doc, revision: document.revision, image: document.previewImage)
            : nil
        let stack = needsCorePreview ? (below: nil as CGImage?, above: nil as CGImage?)
            : transformStackComposites(doc, around: set)
        let layerImage: CGImage?
        let maskImage: CGImage?
        let opacity: CGFloat
        if needsCorePreview {
            layerImage = nil
            maskImage = nil
            opacity = 1
        } else if single, let info = doc.layerInfo(primary) {
            // A hidden layer still transforms; there are simply no pixels to
            // preview, only the box.
            layerImage = info.visible
                ? doc.layerImage(primary)?.makeCGImage(in: doc.colorSpace) : nil
            // Layer pixels come back UNMASKED, so an enabled mask has to
            // clip the preview the way the projection would.
            maskImage = doc.layerMaskEnabled(primary)
                ? doc.layerMaskImage(primary).flatMap(Self.grayMaskImage) : nil
            opacity = CGFloat(info.opacity)
        } else {
            layerImage = transformSetImage(doc, layers: set, rect: rect)
            maskImage = nil
            opacity = 1
        }
        return TransformSession(
            layers: set,
            sourceRect: rect,
            layerImage: layerImage,
            maskImage: maskImage,
            below: stack.below,
            above: stack.above,
            opacity: opacity,
            corePreview: corePreview,
            // A described layer pivots on its description's exact centre
            // (EditorViewController+DescribedTransform.swift), everything
            // else on its box's. A set has no description to pivot on even
            // when one of its members has.
            transform: LayerTransform(
                pivot: single
                    ? (doc.describedPivot(primary) ?? CGPoint(x: rect.midX, y: rect.midY))
                    : CGPoint(x: rect.midX, y: rect.midY)),
            sampler: sampler,
            drag: nil)
    }

    /// The box a session over `layers` opens with: the union of their CONTENT
    /// bounds, falling back to an entry's pixel rect when nothing in it is
    /// opaque (a fully transparent layer still has a buffer to transform, and
    /// a box of nothing would leave no handles to grab). nil when the set has
    /// no rect at all.
    func transformSourceRect(_ doc: RasterDocument, layers: [Int]) -> CGRect? {
        var union: CGRect?
        for idx in layers {
            var rect: CGRect?
            if let bounds = doc.layerBounds(idx), bounds.width > 0, bounds.height > 0 {
                rect = CGRect(
                    x: CGFloat(bounds.x), y: CGFloat(bounds.y),
                    width: CGFloat(bounds.width), height: CGFloat(bounds.height))
            } else if let info = doc.layerInfo(idx), info.width > 0, info.height > 0 {
                rect = CGRect(
                    x: CGFloat(info.offsetX), y: CGFloat(info.offsetY),
                    width: CGFloat(info.width), height: CGFloat(info.height))
            }
            guard let rect = rect else { continue }
            union = union.map { $0.union(rect) } ?? rect
        }
        return union
    }

    /// The set's own pixels, composited once and cut out at `rect` — the
    /// preview plate a multi-entry box drags around.
    ///
    /// Everything outside the set is hidden EXCEPT the set's enclosing
    /// groups: hiding a group hides its whole subtree, so hiding an ancestor
    /// would erase the very layers being previewed. Leaving those ancestors
    /// visible also keeps the preview honest — a member inside a 50 % group
    /// is drawn the way the projection draws it.
    ///
    /// The cut-out is drawn rather than `CGImage.cropping(to:)`, because that
    /// intersects with the image and would silently answer with a SMALLER
    /// image for a box hanging off the canvas — which the canvas would then
    /// stretch across the whole box. Drawing into a `rect`-sized context
    /// always answers at exactly the box's size; the off-canvas part comes
    /// back transparent, which is the truth, since the projection this is cut
    /// from is canvas-sized and holds no pixels out there either.
    private func transformSetImage(_ doc: RasterDocument, layers: [Int], rect: CGRect)
        -> CGImage?
    {
        let width = Int(rect.width)
        let height = Int(rect.height)
        // The same ceiling one transformed layer answers to: a preview is
        // never worth an allocation the commit itself would refuse.
        guard width > 0, height > 0,
              Double(width) * Double(height) <= LayerTransform.maxTransformPixels
        else { return nil }
        let tree = doc.layerTree
        var keep = Set(layers)
        for idx in layers {
            keep.formUnion(tree.ancestors(of: idx))
        }
        var isolated = doc
        for idx in 0..<doc.layerCount where !keep.contains(idx) {
            isolated = isolated.hidingLayer(idx)
        }
        guard let composite = isolated.flattened()?.makeCGImage(in: doc.colorSpace),
              let context = CGContext(
                data: nil, width: width, height: height, bitsPerComponent: 8,
                bytesPerRow: 0, space: doc.colorSpace,
                bitmapInfo: CGImageAlphaInfo.premultipliedLast.rawValue)
        else { return nil }
        // CoreGraphics draws bottom-up while the canvas counts y downward, so
        // the composite's own top-left has to land above the context: row
        // `rect.minY` of the canvas ends up at the top of the cut-out.
        context.draw(
            composite,
            in: CGRect(
                x: -rect.minX, y: rect.height + rect.minY - CGFloat(doc.height),
                width: CGFloat(doc.width), height: CGFloat(doc.height)))
        return context.makeImage()
    }

    // MARK: - Committing a Free Transform session

    /// The per-entry pre-checks a commit runs before it touches the document,
    /// so a refusal can NAME the entry that caused it instead of leaving the
    /// core's bare NULL to become a beep. True when refused.
    ///
    /// Every check mirrors one the core makes (`doc_transform.rs`'s extent
    /// derivation, `doc_align.rs`'s `movable_set`), and the core still makes
    /// them: a path that forgot to ask still cannot write. What this adds is
    /// the sentence.
    ///
    /// The determinant is a property of the matrix and is checked once; the
    /// extent is derived per entry from that entry's OWN rect, because that
    /// is how the core derives it.
    ///
    /// It also ASKS, for every described member other than the primary,
    /// whether its description may be dropped — a transform over several
    /// layers resamples all of them instead of composing into one
    /// description, and `app/CLAUDE.md`'s rule is that every destructive path
    /// asks first. The primary keeps its own prompt in the commit, which
    /// knows whether the compose path was tried. A RAW layer is never
    /// asked: set geometry keeps its retained source. A 16-bit
    /// artwork member is not asked either: the true-perspective
    /// prompt already asked once for the whole set. An 8-bit
    /// true-perspective set asks ONE prompt here over all its described
    /// members, primary included: text and shape, through
    /// `confirmArtworkSetRasterize`. So a set's primary is asked after the
    /// lock and degenerate-matrix checks, not before; an unreadable or
    /// unrenderable artwork member is then refused by the set capture's own
    /// sentence, nothing applied.
    ///
    /// A duplicate session reads `duplicate.source`, whose indices its
    /// `layers` are, and skips the lock check (made at open; any change to
    /// the document makes the base stale); its prompts ask about the COPIES,
    /// and only a copy that carries a description.
    @discardableResult
    func refuseUntransformableSet(_ session: TransformSession, quad: [CGPoint]?) -> Bool {
        guard let document = document, let doc = session.duplicate?.source ?? document.doc
        else { return false }
        return refuseUntransformable(
            layers: session.layers, primary: session.primaryLayer,
            matrix: session.transform.matrix, quad: quad, sourceRect: session.sourceRect,
            in: doc, duplicate: session.duplicate != nil)
    }

    /// `refuseUntransformableSet`'s body over explicit values, so Transform
    /// Again, which opens no session, asks the same checks: `doc` is
    /// the handle `layers` index, and `duplicate` skips the lock check and
    /// asks only about described copies, as a duplicate session does. An
    /// 8-bit true-perspective set's one prompt is asked here, last.
    @discardableResult
    func refuseUntransformable(
        layers: [Int], primary: Int, matrix: CGAffineTransform, quad: [CGPoint]?,
        sourceRect: CGRect, in doc: RasterDocument, duplicate: Bool
    ) -> Bool {
        guard let document = document else { return false }
        if !duplicate,
           refuseLockedEdit(layers: layers, kind: DR_EDIT_POSITION) { return true }
        if abs(matrix.a * matrix.d - matrix.b * matrix.c) < 1e-9 {
            refuseTransform(
                "The transform collapses the layers to a line.",
                "Its matrix is degenerate — a scale of 0 or very near it. "
                    + "Use scales away from 0.")
            return true
        }
        let truePerspective = quad.map {
            LayerTransform.parallelogramAffine(of: sourceRect, onto: $0) == nil
        } ?? false
        for idx in layers {
            // A group has no buffer to resample; what follows the matrix is
            // its canvas-sized mask, which cannot change size. Its raster
            // descendants are in this same set and are checked on their own.
            guard !doc.layerIsGroup(idx), let info = doc.layerInfo(idx),
                  info.width > 0, info.height > 0
            else { continue }
            let rect = CGRect(
                x: CGFloat(info.offsetX), y: CGFloat(info.offsetY),
                width: CGFloat(info.width), height: CGFloat(info.height))
            if quad != nil { continue }
            let extent = matrix.destinationExtent(of: rect)
            guard !LayerTransform.extentIsCommittable(extent) else { continue }
            refuseTransform(
                "Layer “\(info.name)” cannot be transformed that far.",
                "It would become \(Int(extent.width))×\(Int(extent.height)) px, which is "
                    + "either nothing at all, outside the coordinates the core can address, "
                    + "or past its 100 megapixel ceiling for one layer. Scale it down or "
                    + "move it back towards the canvas.")
            return true
        }
        // Last, because it is the only check that asks the user a question:
        // everything that can refuse on its own has refused by now. A
        // duplicate asks only about a copy that carries a description:
        // a RAW-layer copy keeps ⌥⌘T's behaviour.
        if !doc.isHighDepth && layers.count > 1 {
            // An 8-bit set asks ONE prompt over the members it
            // rasterizes, primary included (`describedPrimaryGate` leaves the
            // primary here): every described member under a true perspective.
            // Under an affine or parallelogram nothing is asked, because text
            // and shape layers, the only described kinds, are regenerated.
            let asked = layers.filter {
                truePerspective && !doc.hasRetainedSource($0)
                    && document.layerDescribesSource($0, in: doc)
            }
            if asked.count == 1, let only = asked.first {
                return !document.confirmRasterize(
                    layer: only, reason: Self.setRasterizeReason, in: doc)
            }
            if asked.count >= 2 {
                let names = asked.map { doc.layerInfo($0)?.name ?? "Layer \($0)" }
                return !Self.confirmArtworkSetRasterize(names: names)
            }
            return false
        }
        for idx in layers where idx != primary {
            // Set geometry keeps a RAW layer's retained source bound
            // at both depths and for both map types (`core/tests/
            // retained_geometry.rs`, `core/tests/byte_geometry_cancel.rs`),
            // and MCP `transform_layers` never asked: nothing to rasterize.
            if doc.hasRetainedSource(idx) { continue }
            if duplicate, !document.layerDescribesSource(idx, in: doc) { continue }
            // At 16 bits every caller has already asked about the
            // artwork under a true perspective, once for the whole set
            // (`confirmTruePerspectiveArtwork`, run by
            // `commitPrecisionTransformSession`, `commitDuplicateTransform`,
            // `transformAgainLayers` and `transformAgainCopy`).
            if doc.hasArtworkDescription(idx) && (!truePerspective || doc.isHighDepth) {
                continue
            }
            let allowed = document.confirmRasterize(
                layer: idx, reason: Self.setRasterizeReason, in: doc)
            if !allowed { return true }
        }
        return false
    }

    /// Why a member of a MULTI-entry Free Transform rasterizes even when its
    /// description could render perfectly well: one matrix over several
    /// layers resamples every one of them, because a description can only
    /// absorb a transform of its OWN. Defined once and used at both prompts —
    /// this one for every member but the primary, and the commit's own for
    /// the primary (`EditorViewController.commitTransformSession`), which has
    /// to ask before it gives up the compose path.
    static let setRasterizeReason =
        "It is one of several layers being transformed together, and a transform over "
        + "several layers resamples every one of them instead of composing into a "
        + "description."

    /// The refusal alert both pre-checks share — the shape
    /// `refuseAdjustmentPixelEdit()` and `refuseLockedEdit(layer:kind:)` use,
    /// for the same reason: a transform commit happens on Return or on a tool
    /// switch, outside menu validation's reach.
    private func refuseTransform(_ message: String, _ detail: String) {
        let alert = NSAlert()
        alert.messageText = message
        alert.informativeText = detail
        SessionCommitRefusal.note(alert.messageText)
        alert.runModal()
    }

    // MARK: - Free Transform preview

    /// A mask image (opaque RGBA grayscale, the layer's size) redrawn into a
    /// DeviceGray bitmap, which is the only form CGContext.clip(to:mask:)
    /// accepts. White shows and black hides, matching the core's coverage.
    /// It lives beside the preview it feeds.
    static func grayMaskImage(_ mask: RasterImage) -> CGImage? {
        guard let source = mask.makeCGImage(in: ColorProfile.sRGB),
              source.width > 0, source.height > 0,
              let context = CGContext(
                data: nil, width: source.width, height: source.height,
                bitsPerComponent: 8, bytesPerRow: source.width,
                space: CGColorSpaceCreateDeviceGray(),
                bitmapInfo: CGImageAlphaInfo.none.rawValue)
        else { return nil }
        context.draw(
            source, in: CGRect(x: 0, y: 0, width: source.width, height: source.height))
        return context.makeImage()
    }

    /// The two composites the preview draws the transformed set between: the
    /// stack below it and the stack above it. Built once per session, so the
    /// drag itself never calls the core.
    ///
    /// `below` shows every entry beneath the LOWEST one being transformed;
    /// `above` shows everything else that is not in the set. With a
    /// CONTIGUOUS selection — a single layer included — that is exactly the
    /// sandwich it has always been.
    ///
    /// With a NON-CONTIGUOUS selection there is no correct two-plate
    /// sandwich: an unselected layer sitting between two selected ones is
    /// both above one and below another. The decision, stated here because
    /// the picture cannot show it, is that such a layer draws in the `above`
    /// plate — so it covers the preview rather than vanishing from it. The
    /// transform itself is exact either way; only the preview's ordering is
    /// approximate.
    ///
    /// The `below` plate keeps the set's ENCLOSING GROUPS visible, because
    /// hiding a group hides its whole subtree: a group entry closes its own
    /// subtree, so an ancestor's index is always ABOVE its children's, and
    /// the sweep that hides everything from the lowest member upward would
    /// otherwise hide the ancestor — taking the transformed layer's own
    /// SIBLINGS BELOW IT down with it, so they would vanish from the
    /// backdrop the moment ⌘T opened. Left visible, the ancestor renders
    /// only the children the sweep left visible, which is exactly the half
    /// of the group the plate wants, at the group's own opacity and mask.
    /// `above` needs no such exception: an ancestor is above the lowest
    /// member and is not in the set, so that sweep already leaves it alone.
    /// In a document with no groups there are no ancestors and both plates
    /// are byte-for-byte what they have always been.
    func transformStackComposites(_ doc: RasterDocument, around layers: [Int])
        -> (below: CGImage?, above: CGImage?)
    {
        guard let low = layers.min(), low >= 0, low < doc.layerCount else { return (nil, nil) }
        let set = Set(layers)
        let tree = doc.layerTree
        // A member's own subtree is already in `set`, so an ancestor that is
        // itself a member stays hidden with the rest of it.
        var enclosing = Set<Int>()
        for idx in layers {
            enclosing.formUnion(tree.ancestors(of: idx))
        }
        enclosing.subtract(set)
        var belowDoc = doc
        for layer in low..<doc.layerCount where !enclosing.contains(layer) {
            belowDoc = belowDoc.hidingLayer(layer)
        }
        var aboveDoc = doc
        for layer in 0..<doc.layerCount where layer <= low || set.contains(layer) {
            aboveDoc = aboveDoc.hidingLayer(layer)
        }
        // Nothing above the set at all: no plate rather than a transparent
        // one, which is what the canvas checks for. An enclosing group with
        // every child hidden composites to nothing, so it counts as nothing
        // above rather than as a reason to build an empty plate.
        let nothingAbove = (low + 1..<doc.layerCount).allSatisfy {
            set.contains($0) || enclosing.contains($0)
        }
        return (
            belowDoc.flattened()?.makeCGImage(in: doc.colorSpace),
            nothingAbove ? nil : aboveDoc.flattened()?.makeCGImage(in: doc.colorSpace))
    }

    /// The one-entry form, which is what a session opened on a single layer
    /// asks for.
    func transformStackComposites(_ doc: RasterDocument, around idx: Int)
        -> (below: CGImage?, above: CGImage?)
    {
        transformStackComposites(doc, around: [idx])
    }

    /// Pushes the session's current matrix (and the box derived from it) to
    /// the canvas. Cheap enough to run on every drag tick.
    func refreshTransformPreview() {
        guard let session = transformSession else {
            canvas.transformPreview = nil
            return
        }
        if session.floating != nil {
            refreshFloatingTransformPreview(session)
            return
        }
        if session.precisionGesture != nil {
            refreshPrecisionTransformPreview(session)
            return
        }
        if session.corePreview != nil {
            refreshMaskedTransformPreview(session)
            return
        }
        canvas.transformPreview = ImageCanvasView.TransformPreview(
            below: session.below,
            above: session.above,
            layer: session.layerImage,
            mask: session.maskImage,
            sourceRect: session.sourceRect,
            matrix: session.transform.matrix,
            opacity: session.opacity,
            quad: session.transform.warpedQuad(of: session.sourceRect),
            pivot: session.transform.pivotInCanvas,
            interpolate: session.sampler != DR_FILTER_NEAREST,
            warped: session.transform.hasCornerOffsets)
    }
}

extension RasterDocument {
    /// The given entries expanded the way every "move this" op expands them:
    /// by SUBTREE (moving a group moves its children) and then by LINK GROUP
    /// (every entry carrying a member's non-zero link, with its own subtree),
    /// to a fixed point, deduplicated and ascending.
    ///
    /// The host's copy of `doc_group_query::expanded_set`, and deliberately
    /// only that: it decides what the transform BOX encloses, what the
    /// preview hides and which entry a refusal names. The core still expands
    /// the set itself inside `move_layers` / `transform_layers`, so the two
    /// can disagree only about wording, never about pixels.
    ///
    /// A fixed point rather than one pass, because a linked entry's subtree
    /// can contain an entry that is itself linked into a further group; the
    /// loop is bounded by the entry count, which no growth step can exceed.
    func movingSet(_ indices: [Int]) -> [Int] {
        let count = layerCount
        guard count > 0 else { return [] }
        let tree = layerTree
        // Read every link id ONCE: the fixed point below can revisit an entry
        // several times, and each read is an FFI call.
        let links = (0..<count).map { layerLink($0) }
        var included = Set<Int>()
        for idx in indices where idx >= 0 && idx < count {
            included.formUnion(tree.subtree(of: idx))
        }
        for _ in 0..<count {
            let joined = Set(included.map { links[$0] }.filter { $0 != 0 })
            guard !joined.isEmpty else { break }
            var grew = false
            for idx in 0..<count where !included.contains(idx) && joined.contains(links[idx]) {
                included.formUnion(tree.subtree(of: idx))
                grew = true
            }
            if !grew { break }
        }
        return included.sorted()
    }

    /// Which of `set`'s entries carry a DESCRIPTION, by the kind their meta
    /// claims — ascending, so a report reads in stack order.
    ///
    /// Read BEFORE the edit, because afterwards the descriptions are gone and
    /// nothing could name what was dropped.
    func describedEntries(in set: [Int]) -> [(layer: Int, kind: DescribedKind)] {
        set.sorted().compactMap { idx in
            LayerDescription.claimedKind(of: layerMeta(idx)).map { (layer: idx, kind: $0) }
        }
    }

    /// `self` with the description dropped from every entry in `set`.
    ///
    /// The ONE implementation of `app/CLAUDE.md`'s invariant for a SET edit:
    /// a resample rewrites the pixels a described layer was rendered from, so
    /// the meta — which must always describe the CURRENT pixels — goes in the
    /// same edit. It matters most where the set is not the caller's list: a
    /// transform aimed at a GROUP, or at one member of a LINK group,
    /// resamples every entry `movingSet` expands to, and leaving their
    /// descriptions behind would let `edit_text_layer` (or a double-click on
    /// the row) re-render a child from a description that predates the
    /// transform and snap it back to where it used to be.
    func droppingDescriptions(in set: [Int]) -> RasterDocument {
        var result = self
        for idx in set {
            result = result.withLayerMeta(idx, nil) ?? result
        }
        return result
    }
}
