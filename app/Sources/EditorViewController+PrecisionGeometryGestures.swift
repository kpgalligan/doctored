import AppKit

// A lone text or shape layer under an affine or parallelogram ⌘T goes through `.transformSet` so
// its artwork is captured and regenerated; a true perspective asks one prompt for the whole set
// (`confirmTruePerspectiveArtwork`), and `refuseUntransformable`'s loop skips RAW layers at both
// depths and artwork members at 16 bits. At 8 bits a true-perspective set asks ONE prompt over its
// described members, primary included, from `refuseUntransformable` (`describedPrimaryGate` leaves
// a set's primary to it); an affine or parallelogram set asks nothing, because text and shape
// layers, the only described kinds, are regenerated from their descriptions, and
// `describedPrimaryGate` leaves EVERY 8-bit set's primary to `refuseUntransformable`.

extension EditorViewController {
    func precisionGeometryDocumentDidChange() {
        precisionMoveGesture?.invalidateIfStale()
        if !isCommittingTransform {
            transformSession?.precisionGesture?.invalidateIfStale()
        }
        if !isCommittingPrecisionCrop {
            cropSession?.precisionGesture?.invalidateIfStale()
        }
    }

    /// `display`/`initialImage`: the plane move previews the targeted
    /// mask or channel when the canvas shows it as its base.
    /// `workSource`: a continued Move float's source.
    func beginPrecisionMoveGesture(_ document: ImageDocument,
                                   display: ((RasterDocument) -> CGImage?)? = nil,
                                   initialImage: CGImage? = nil,
                                   workSource: RasterDocument? = nil) {
        do {
            let gesture = try PrecisionGeometryGesture(
                document, display: display, initialImage: initialImage, workSource: workSource,
                indexedBackground: backgroundColor)
            precisionMoveGesture = gesture
            canvas.precisionMoveActive = true
            gesture.onUpdate = { [weak self, weak gesture] in
                guard let self, let gesture, self.precisionMoveGesture === gesture else { return }
                self.canvas.previewImage = gesture.image
                self.canvas.needsDisplay = true
            }
        } catch {
            moveAppliedDelta = nil
            document.presentError(error)
        }
    }

    func updatePrecisionMoveGesture(dx: Int, dy: Int) {
        guard let gesture = precisionMoveGesture else { return }
        gesture.request(.move(layers: gesture.capture.selectedLayers, dx: dx, dy: dy))
    }

    /// The mouse-up and the tool-switch cancellation have distinct results.
    /// A typed preview never opened ImageDocument's live-edit transaction.
    func moveDidFinish() {
        if moveSelectedSession != nil {
            finishMoveSelected()
            return
        }
        if moveDuplicate != nil {
            finishMoveDuplicate()
            return
        }
        guard let document else { return }
        let applied = moveAppliedDelta
        moveAppliedDelta = nil
        moveDidEnd()
        if let gesture = precisionMoveGesture {
            precisionMoveGesture = nil
            canvas.precisionMoveActive = false
            defer { gesture.cancel(); canvas.previewImage = nil }
            do {
                let delta = applied ?? (0, 0)
                try gesture.finish(
                    .move(layers: gesture.capture.selectedLayers, dx: delta.x, dy: delta.y),
                    name: "Move Layer",
                    record: Self.moveRecord(gesture.capture.facade,
                                            gesture.capture.selectedLayers,
                                            dx: delta.x, dy: delta.y))
            } catch { document.presentError(error) }
            return
        }
        guard document.doc?.isHighDepth != true else { return }
        // A pattern fill the ticks moved is re-rendered once, in the same step.
        guard refitLiveMove(document) else { return }
        document.endLiveEdit("Move Layer", record: Self.moveEndRecord(document, delta: applied ?? (0, 0)))
    }

    func moveDidCancel() {
        if moveSelectedSession != nil {
            cancelMoveSelected()
            return
        }
        if let gesture = precisionMoveGesture {
            precisionMoveGesture = nil
            moveDuplicate = nil
            gesture.cancel()
            canvas.previewImage = nil
            canvas.precisionMoveActive = false
            moveAppliedDelta = nil
            moveDidEnd()
        } else {
            moveDidFinish()
        }
    }

    func precisionMoveNudge(indices: [Int], dx: Int, dy: Int) {
        guard let document, let source = document.doc else { return }
        do {
            let gesture = try PrecisionGeometryGesture(document, indexedBackground: backgroundColor)
            defer { gesture.cancel() }
            try gesture.finish(.move(layers: indices, dx: dx, dy: dy),
                               name: "Move Layer",
                               record: Self.moveRecord(source, indices, dx: dx, dy: dy))
        } catch { document.presentError(error) }
    }

    /// ⌘T captures the whole expanded set for the native handle session.
    func beginPrecisionTransformSession() {
        guard let document, let doc = document.doc else { return }
        let expanded = doc.movingSet(document.selectedLayerIndices)
        guard !expanded.isEmpty else { return }
        guard !refuseLockedEdit(layers: expanded, kind: DR_EDIT_POSITION) else { return }
        guard var session = makeTransformSession(doc, sampler: transformSampler) else {
            NSSound.beep()
            return
        }
        do {
            let gesture = try PrecisionGeometryGesture(document, indexedBackground: backgroundColor)
            session.precisionGesture = gesture
            transformSession = session
            updateOptionsBar()
            refreshTransformPreview()
            optionsBar.refreshValues()
            updateStatus()
            view.window?.makeFirstResponder(canvas)
        } catch { document.presentError(error) }
    }

    func precisionTransformRequest(_ session: TransformSession) -> PrecisionGeometryRequest {
        let quad = session.transform.hasCornerOffsets
            ? session.transform.warpedQuad(of: session.sourceRect) : nil
        // A duplicate session's layers index its duplicated source:
        // answered before the document reads below.
        if let copy = session.duplicate {
            return .duplicateTransform(layers: copy.originals, sourceRect: session.sourceRect,
                                       matrix: session.transform.matrix, quad: quad,
                                       sampler: session.sampler)
        }
        // A lone text or shape layer under an affine or parallelogram
        // goes through `.transformSet` too. The typed `.transform` clears the
        // description (`core/src/precision_geometry.rs`), while the set
        // captures and regenerates the artwork (`PrecisionGeometryWork.init`),
        // which is 16-bit ⇧⌘T's and `transform_layers`' rule. A true
        // perspective stays `.transform`, after `confirmTruePerspectiveArtwork`'s
        // prompt. Artwork that cannot render (a missing font) refuses with
        // `ArtworkGeometryWork`'s sentence and keeps the session open, as the
        // 8-bit ⌘T, 16-bit sets and ⇧⌘T already do.
        let truePerspective = quad.map {
            LayerTransform.parallelogramAffine(of: session.sourceRect, onto: $0) == nil
        } ?? false
        if !session.isSingleLayer || session.layers.contains(where: {
            document?.doc?.layerIsGroup($0) == true
                || document?.doc?.layerIsAdjustment($0) == true
        }) || (document?.doc?.hasArtworkDescription(session.primaryLayer) == true
               && !truePerspective) {
            return .transformSet(layers: session.layers, sourceRect: session.sourceRect,
                                 matrix: session.transform.matrix, quad: quad,
                                 sampler: session.sampler)
        }
        return .transform(layer: session.primaryLayer, matrix: session.transform.matrix,
                          quad: quad, sampler: session.sampler)
    }

    /// `request` replaces the session's own layer request — the floating
    /// session passes `.transformSelected`.
    func refreshPrecisionTransformPreview(_ session: TransformSession,
                                          request: PrecisionGeometryRequest? = nil) {
        guard let gesture = session.precisionGesture else { return }
        gesture.onUpdate = { [weak self, weak gesture] in
            guard let self, let gesture, let current = self.transformSession,
                  current.precisionGesture === gesture else { return }
            self.canvas.transformPreview = ImageCanvasView.TransformPreview(
                composite: gesture.image, usesCoreComposite: true,
                below: nil, above: nil, layer: nil, mask: nil,
                sourceRect: current.sourceRect, matrix: current.transform.matrix,
                opacity: 1, quad: current.transform.warpedQuad(of: current.sourceRect),
                pivot: current.transform.pivotInCanvas,
                interpolate: current.sampler != DR_FILTER_NEAREST,
                warped: current.transform.hasCornerOffsets)
            self.updateStatus()
        }
        gesture.request(request ?? precisionTransformRequest(session))
        gesture.onUpdate?()
    }

    /// A true perspective rasterizes editable artwork: ONE prompt per set
    /// over the artwork entries of `doc.movingSet(layers)`, ⌘T's
    /// 16-bit rule, shared with Transform Again's perspective replay.
    /// A lone entry keeps its kind-specific prompt; two or more share
    /// `confirmArtworkSetRasterize`, and an unreadable or unrenderable member
    /// is then refused by the set capture's own sentence, nothing applied.
    /// `refuseUntransformable`'s loop never re-asks at 16 bits. False when
    /// the prompt is declined.
    func confirmTruePerspectiveArtwork(_ doc: RasterDocument, layers: [Int]) -> Bool {
        guard let document else { return false }
        let artwork = doc.movingSet(layers).filter(doc.hasArtworkDescription)
        if artwork.count >= 2 {
            return Self.confirmArtworkSetRasterize(
                names: artwork.map { doc.layerInfo($0)?.name ?? "Layer \($0)" })
        }
        for index in artwork {
            guard document.confirmRasterize(layer: index,
                reason: "True perspective will rasterize this editable artwork layer.",
                in: doc) else {
                return false
            }
        }
        return true
    }

    /// The one prompt for two or more artwork layers under a true
    /// perspective; true = Rasterize. A decline notes its refusal. It is
    /// also an 8-bit true-perspective set's prompt
    /// (`refuseUntransformable`).
    static func confirmArtworkSetRasterize(names: [String]) -> Bool {
        confirmSetRasterize(
            count: names.count, noun: "editable artwork layers",
            informative: "True perspective will rasterize \(setRasterizeList(names)). They "
                + "will no longer be editable as text or shape: the descriptions they were "
                + "rendered from are dropped. The pixels themselves are kept.")
    }

    /// The set prompts' member list: the first five names quoted, then
    /// " and N more".
    private static func setRasterizeList(_ names: [String]) -> String {
        var list = names.prefix(5).map { "“\($0)”" }.joined(separator: ", ")
        if names.count > 5 { list += " and \(names.count - 5) more" }
        return list
    }

    /// The set prompts' alert: Rasterize / Cancel, and a decline's note.
    private static func confirmSetRasterize(count: Int, noun: String, informative: String) -> Bool {
        let alert = NSAlert()
        alert.messageText = "Rasterize \(count) \(noun)?"
        alert.informativeText = informative
        alert.addButton(withTitle: "Rasterize")
        alert.addButton(withTitle: "Cancel")
        let confirmed = alert.runModal() == .alertFirstButtonReturn
        if !confirmed {
            SessionCommitRefusal.note(
                "The prompt to rasterize \(count) \(noun) was cancelled in the "
                    + "document window.")
        }
        return confirmed
    }

    @discardableResult
    func commitPrecisionTransformSession(_ session: TransformSession) -> Bool {
        guard let document, let gesture = session.precisionGesture else { return false }
        do { try gesture.capture.requireCurrent(document) }
        catch { document.presentError(error); return false }
        guard !session.transform.isIdentity else {
            endTransformSession()
            return true
        }
        let quad = session.transform.hasCornerOffsets
            ? session.transform.warpedQuad(of: session.sourceRect) : nil
        if let quad,
           LayerTransform.parallelogramAffine(of: session.sourceRect, onto: quad) == nil,
           let source = document.doc {
            guard confirmTruePerspectiveArtwork(source, layers: session.layers) else { return false }
        }
        guard !refuseUntransformableSet(session, quad: quad) else { return false }
        isCommittingTransform = true
        defer { isCommittingTransform = false }
        do {
            try gesture.finish(precisionTransformRequest(session),
                               name: "Transform Layer", record: transformRecord(session, quad: quad))
            endTransformSession()
            return true
        } catch is JobCancelled {
            // The gesture re-armed itself: keep the session, ask again.
            SessionCommitRefusal.note(JobCancelled.message)
            refreshTransformPreview()
            return false
        } catch {
            document.presentError(error)
            return false
        }
    }

    /// An extended box previews with its Background fill and its Delete Cropped Pixels
    /// (forced by Content-Aware Fill), so a turned box's corners show the pad the commit
    /// writes; a box inside the canvas keeps neither.
    func refreshPrecisionCropPreview(_ session: CropSession) {
        guard let gesture = session.precisionGesture else { return }
        let facade = gesture.capture.facade
        let canvas = CGSize(width: facade.width, height: facade.height)
        var request = PrecisionGeometryRequest.crop(rect: session.rect, angle: session.angle,
                                                     sampler: transformSampler,
                                                     deleteCropped: false)
        if CropSession.extends(CropSession.roundedWindow(session.rect, canvas: canvas), canvas: canvas) {
            let options = ToolOptionsStore.shared
            request = .crop(
                rect: session.rect, angle: session.angle, sampler: transformSampler,
                deleteCropped: options.crop.deleteCroppedPixels
                    || CropFill(index: options.crop.fillIndex) == .contentAware,
                background: try? CropBackground.make(
                    backgroundColor, cmykInk: options.sharedState.backgroundInk,
                    labInk: options.sharedState.backgroundLab, in: facade))
        }
        gesture.onUpdate = { [weak self, weak gesture] in
            guard let self, let gesture, self.cropSession?.precisionGesture === gesture else { return }
            let window = CropSession.roundedWindow(self.cropSession?.rect ?? session.rect,
                canvas: CGSize(width: gesture.capture.facade.width,
                               height: gesture.capture.facade.height))
            self.canvas.precisionCropWindow = window
            // A reduced preview is the window at the proxy's size; the
            // canvas draws either into the window.
            if !gesture.isPending, let image = gesture.image,
               gesture.imageIsReduced
                   || (image.width == Int(window.width) && image.height == Int(window.height)) {
                self.canvas.precisionCropPreview = image
            } else {
                self.canvas.precisionCropPreview = nil
            }
            self.canvas.needsDisplay = true
        }
        gesture.request(request)
        gesture.onUpdate?()
    }

    func commitPrecisionCropSession(_ session: CropSession) {
        guard let document, let gesture = session.precisionGesture else { return }
        do { try gesture.capture.requireCurrent(document) }
        catch { document.presentError(error); return }
        let source = gesture.capture.facade
        let rect = CropSession.roundedWindow(session.rect,
            canvas: CGSize(width: source.width, height: source.height))
        guard rect.width >= 1, rect.height >= 1 else {
            document.presentError(DoctoredCoreError(message: "Crop needs a nonempty window."))
            return
        }
        let full = rect.minX == 0 && rect.minY == 0
            && rect.width == CGFloat(source.width) && rect.height == CGFloat(source.height)
        guard session.angle != 0 || !full else { return }
        // A window past the canvas carries the Background fill, after
        // Canvas Size's own size refusals.
        let extended = CropSession.extends(
            rect, canvas: CGSize(width: source.width, height: source.height))
        var background: CropBackground?
        if extended {
            if let refusal = source.extendedCropRefusal(width: Int(rect.width),
                                                        height: Int(rect.height)) {
                document.presentError(DoctoredCoreError(message: refusal))
                return
            }
            do {
                background = try CropBackground.make(
                    backgroundColor, cmykInk: ToolOptionsStore.shared.sharedState.backgroundInk,
                    labInk: ToolOptionsStore.shared.sharedState.backgroundLab, in: source)
            } catch {
                document.presentError(error)
                return
            }
        }
        let sampler = transformSampler
        let deleteCropped = ToolOptionsStore.shared.crop.deleteCroppedPixels
        // A Fill other than Background (Default) runs its own job on the crop.
        let fill = CropFill(index: ToolOptionsStore.shared.crop.fillIndex)
        if extended, fill != .background, let background {
            commitCropFill(CropFillRequest(
                rect: rect, angle: session.angle, sampler: sampler,
                deleteCropped: deleteCropped || fill == .contentAware, background: background,
                described: session.angle == 0
                    ? [] : (0..<source.layerCount).filter { document.layerDescribesSource($0) }),
                fill: fill)
            return
        }
        isCommittingPrecisionCrop = true
        defer { isCommittingPrecisionCrop = false }
        do {
            let outcome = try gesture.finishOutcome(
                .crop(rect: session.rect, angle: session.angle, sampler: sampler,
                      deleteCropped: deleteCropped, background: background),
                name: "Crop",
                record: .crop(rect, angle: session.angle,
                              sampler: Self.samplerAgentName(sampler),
                              deleteCropped: deleteCropped,
                              backgroundColor: extended ? backgroundColor : nil),
                changesCanvasGeometry: true,
                // The selection goes through the crop's own map.
                resultingSelection: document.selectionCarried(
                    by: CropSession.canvasMap(window: rect, angle: session.angle),
                    width: Int(rect.width), height: Int(rect.height)))
            switch outcome {
            case .published:
                beginCropSession()
                optionsBar.refreshValues()
            case .unchanged, .refused:
                // Only a full window turned by a nonzero angle answers
                // `.unchanged`; `applyEdit` beeped for `.refused`.
                if outcome == .unchanged {
                    SessionCommitRefusal.beep(CropSession.unchangedStraightenSentence(
                        width: source.width, height: source.height, angle: session.angle))
                }
                rearmPrecisionCropSession(session, document: document)
            }
        } catch is JobCancelled {
            // The gesture re-armed itself: keep the crop box, ask again.
            refreshPrecisionCropPreview(cropSession ?? session)
        } catch {
            document.presentError(error)
            endCropSession()
        }
    }

    /// The session is kept after `.unchanged` / `.refused`, but `finishOutcome`
    /// deactivated its gesture: a fresh one makes later box edits preview.
    func rearmPrecisionCropSession(_ session: CropSession, document: ImageDocument) {
        var kept = cropSession ?? session
        kept.precisionGesture?.cancel()
        do {
            kept.precisionGesture = try PrecisionGeometryGesture(
                document, indexedBackground: backgroundColor)
        }
        catch { document.presentError(error); return }
        cropSession = kept
        refreshPrecisionCropPreview(kept)
    }
}
