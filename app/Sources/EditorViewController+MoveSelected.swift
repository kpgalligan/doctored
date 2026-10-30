import AppKit

// `moveDidBegin` routes to Move Selected BEFORE the Position-lock refusal; the press opens nothing
// (`MoveSelectedSession`), the first non-zero tick runs the Pixels-lock owner alert,
// `MoveSelectedPixels.refusal` (Quick Mask, the shared `multiLayerRefusal(verb:count:)` ⌘T reuses,
// group/adjustment, non-pixel target, `.pixelOnly`; a float document is admitted) and `confirmRasterize`, then opens a
// latest-wins background `TransformMaskPreview` (8-bit; its generic
// `request(key:identity:scratch:compute:)`) or the typed `PrecisionGeometryGesture`
// (`.moveSelected`).
//
// ⌥ at the press is latched (`moveDidBegin` → `beginMoveSelected(_:duplicate:)`) and makes
// the drag a DUPLICATE (`"duplicate":true`, no hole, one "Duplicate" step, the selection on the
// copy; MCP `duplicate`).
//
// A successful RECT answer on the current, opened, unrefused session becomes `movePressBox`,
// so the drag snaps on the float's opaque bounds (earlier ticks snap on selection ∩ layer); the
// 8-bit commit's waits and `SelectedPixelsFloat.bounds` pass `revealAfter:
// DocumentJobs.panelDelay`, so a short wait never flashes its panel. ⌥+arrow over a selection
// is the drag's duplicate (`moveNudge(_:_:duplicate:)` → `moveSelectedNudge(_:_:duplicate:)`, one
// "Duplicate" step per press; the canvas's nudge guard excludes only ⌘ and ⌃), and the Move tool's
// cursor is `.dragCopy` with ⌥ held (`selectionFeedbackCursor`) and through an ⌥ drag
// (`moveDragDuplicate` latched at the press, `moveDragCursor`).
//
// A LAYER MASK or ALPHA CHANNEL edit target (latched at the press) takes the plane branch,
// ⌘T's rules: `MoveSelectedPixels.planeRefusal` instead of the Pixels-lock alert and
// `refusal`, the selection's bounds as the box, the background swatch as the hole, no float
// query, no rasterize prompt, and the targeted plane as the preview when the canvas shows it.
// A RED, GREEN, BLUE or ALPHA target takes the same branch plus the layer path's lock owner
// alert (`MoveSelectedPixels.planeLockKind`) and rasterize prompt (the 8-bit source drops the
// description); its hole is the background swatch's 30/59/11 luma rounded to a code, Alpha's
// transparent.
//
// A plain drag or nudge over a live `MoveFloat` (`ImageDocument.liveMoveFloat`) continues
// it: the source, coverage, bounds and background are the float's, every request is the float's
// TOTAL from its source (`start` + the drag), there is no rasterize prompt, and a total of (0, 0)
// publishes the source itself (`publishMoveFloatOrigin`). Each publication notes the float
// (`noteMoveStep`); a fresh lift starts one. ⌥ over a live float lifts a fresh copy (A-STAMP).

/// Move Selected Pixels: the Move tool's drag and arrow nudge with a
/// selection active. The model, refusals and the 8-bit commit live in
/// `MoveSelectedPixels.swift`; this file is the controller half.
///
/// The drag: the press captures the document, the active layer and the
/// selection and opens nothing, so a click stays inert. The first non-zero
/// tick runs the refusals and the rasterize confirmation, then opens the
/// preview — a latest-wins background `TransformMaskPreview` at 8 bits, the
/// typed `PrecisionGeometryGesture` at 16 — and every tick asks for the TOTAL
/// delta from the captured source, never chained. The marquee moves at once;
/// the pixels follow as each preview lands. The release commits ONE "Move"
/// step carrying the moved selection.
///
/// ⌥ at the press latches a DUPLICATE: the same drag with no hole,
/// committed as ONE "Duplicate" step with the selection on the copy. The
/// first tick also starts a background float-rect query at both depths; an
/// EMPTY answer tears the preview down and refuses by name mid-drag.
extension EditorViewController {
    func beginMoveSelected(_ document: ImageDocument, duplicate: Bool) {
        guard let doc = document.doc, let selection = document.canvasSelection else { return }
        let layer = document.activeLayerIndex
        let target = document.planeEditTarget
        var session = MoveSelectedSession(
            base: doc, layer: layer, selection: document.selectionHistory, duplicate: duplicate)
        session.target = target
        // A plain press over a live float continues it; ⌥ stamps a fresh copy.
        if !duplicate, let continuing = document.liveMoveFloat(layer: layer, target: target) {
            let position = continuing.float.positions[continuing.at]
            session.continuing = continuing
            session.start = (position.dx, position.dy)
        }
        moveSelectedSession = session
        var box = selection.bounds
        if target == .layer, let info = doc.layerInfo(layer) {
            let rect = CGRect(x: info.offsetX, y: info.offsetY, width: info.width, height: info.height)
            let clipped = box.intersection(rect)
            if !clipped.isNull, clipped.width >= 1, clipped.height >= 1 { box = clipped }
        }
        movePressBox = box
        moveAppliedDelta = nil
    }

    func updateMoveSelected(dx: Int, dy: Int) {
        guard var session = moveSelectedSession, let document, !session.refused else { return }
        if !session.opened {
            guard dx != 0 || dy != 0 else { return }
            session.opened = true
            let opened = openMoveSelected(&session, document)
            session.refused = !opened
            // A window closed during a modal prompt dropped the session.
            guard moveSelectedSession != nil else { return }
            moveSelectedSession = session
            guard opened else { return }
        }
        session.applied = (dx, dy)
        moveSelectedSession = session
        // The marquee is already at a continued float's start: it moves by the drag.
        canvas.moveSelectionOffset = CGPoint(x: dx, y: dy)
        guard let command = try? moveSelectedCommand(session, dx: dx, dy: dy) else { return }
        let total = (x: session.start.x + dx, y: session.start.y + dy)
        if let preview = session.preview, let source = session.source {
            // A continued float back at its lift shows its source.
            if session.continuing != nil, total.x == 0, total.y == 0 {
                preview.request(key: [0, 0], identity: false, scratch: { 0 }) { _ in source }
                return
            }
            let request = command.request
            let coverage = session.coverage
            let budget = session.budget
            // The plan of THIS request, bounded by the selection.
            preview.request(key: [Double(total.x), Double(total.y)], identity: dx == 0 && dy == 0,
                            scratch: {
                try PrecisionDocumentDescriptor.integer(
                    source.selectedPixelsPlan(request: request), "scratch_bytes")
            }) { control in
                guard let moved = try source.editingSelectedPixels(
                    request: request, coverage: coverage, budget: budget, control: control)
                else { throw DoctoredCoreError(message: "Could not preview the move.") }
                return moved
            }
            return
        }
        precisionMoveGesture?.request(.moveSelected(command: command, coverage: session.coverage))
    }

    /// The first non-zero tick: refusals, the rasterize prompt, the stale-base
    /// guard, then the preview. False when refused (already presented).
    private func openMoveSelected(_ session: inout MoveSelectedSession,
                                  _ document: ImageDocument) -> Bool {
        if session.target != .layer { return openMoveSelectedPlane(&session, document) }
        let layer = session.layer
        // A continued float lifts from its own source, which already
        // dropped any description, so it asks no rasterize prompt.
        let float = session.continuing?.float
        if refuseMoveSelected(document, layer: layer) { return false }
        if float == nil, document.layerDescribesSource(layer) {
            guard document.confirmRasterize(layer: layer) else { return false }
            session.described = true
        }
        // The prompt is a modal that services MCP: a base another edit
        // replaced would publish pixels and a selection that disagree.
        guard document.doc === session.base, document.selectionHistory == session.selection,
              let selection = session.selection.selection else {
            document.presentError(DoctoredCoreError(message: MoveSelectedPixels.staleSentence))
            return false
        }
        session.coverage = float?.coverage ?? selection.maskSamples16()
        // Both depths: an empty float refuses as soon as this answers.
        let query = SelectedPixelsFloatQuery(
            source: float?.source ?? session.base, layer: layer, coverage: session.coverage)
        query.onResult = { [weak self, weak query] rect in
            guard let self, let query else { return }
            self.moveSelectedFloatAnswered(rect, query: query)
        }
        session.floatQuery = query
        query.start()
        if session.base.isHighDepth {
            beginPrecisionMoveGesture(document, workSource: float?.source)
            if precisionMoveGesture == nil { query.cancel() }
            return precisionMoveGesture != nil
        }
        let source: RasterDocument
        do {
            if let float {
                source = float.source
            } else if session.described {
                guard let dropped = session.base.withLayerMeta(layer, nil) else {
                    throw DoctoredCoreError(message: "The layer's description could not be dropped.")
                }
                source = dropped
            } else {
                source = session.base
            }
            session.budget = try session.base.legacyPrecisionWorkContext()
            // Open-time validation only; each preview request plans itself.
            _ = try source.selectedPixelsPlan(request: float?.command(
                dx: session.start.x, dy: session.start.y).request
                ?? MoveSelectedPixels.command(
                    layer: layer, dx: 0, dy: 0, duplicate: session.duplicate,
                    bounds: selection.bounds).request)
        } catch {
            query.cancel()
            document.presentError(error)
            return false
        }
        session.source = source
        let preview = TransformMaskPreview(
            source: source, revision: document.revision, image: document.previewImage)
        preview.onUpdate = { [weak self, weak preview] in
            guard let self, let preview, self.moveSelectedSession?.preview === preview else { return }
            self.canvas.previewImage = preview.image
            self.canvas.needsDisplay = true
        }
        session.preview = preview
        canvas.precisionMoveActive = true
        return true
    }

    /// The first tick of a mask or channel move, ⌘T's plane rules —
    /// the plane refusal, the selection's box, the background swatch, the
    /// stale guard, the coverage, then the preview. No float query, no
    /// rasterize prompt, no description dropped: the layer's pixels stay.
    /// A colour plane first takes the layer path's lock alert and,
    /// after the refusals, its rasterize prompt.
    private func openMoveSelectedPlane(_ session: inout MoveSelectedSession,
                                       _ document: ImageDocument) -> Bool {
        let layer = session.layer
        let target = session.target
        // A continued float keeps its source, coverage and background.
        let float = session.continuing?.float
        if let kind = MoveSelectedPixels.planeLockKind(target),
           refuseLockedEdit(layer: layer, kind: kind) { return false }
        if let reason = MoveSelectedPixels.planeRefusal(
            document, layer: layer, target: target, verb: "move",
            subject: "Moving selected pixels") {
            document.presentError(DoctoredCoreError(message: reason))
            return false
        }
        guard let captured = session.selection.selection,
              TransformSelectedPixels.planeBox(captured) != nil else {
            document.presentError(DoctoredCoreError(message: MoveSelectedPixels.emptyRefusal))
            return false
        }
        if float == nil, target.isPlane, document.layerDescribesSource(layer) {
            guard document.confirmRasterize(layer: layer) else { return false }
            session.described = true
        }
        if let float {
            session.background = float.background
        } else {
            let hex = ActionArgs.hex(backgroundColor)
            do {
                session.background = try TransformSelectedPixels.planeBackground(
                    hex: hex, target: target, in: document)
            } catch {
                document.presentError(error)
                return false
            }
        }
        guard document.doc === session.base, document.selectionHistory == session.selection else {
            document.presentError(DoctoredCoreError(message: MoveSelectedPixels.staleSentence))
            return false
        }
        session.coverage = float?.coverage ?? captured.maskSamples16()
        let display: ((RasterDocument) -> CGImage?)? =
            canvas.channelDisplay?.baseKind.coverageTarget == target
            ? TransformSelectedPixels.planeDisplay(target: target, layer: layer) : nil
        let initial = display == nil ? nil : canvas.channelDisplay?.base
        if session.base.isHighDepth {
            beginPrecisionMoveGesture(document, display: display, initialImage: initial,
                                      workSource: float?.source)
            return precisionMoveGesture != nil
        }
        let source: RasterDocument
        do {
            if let float {
                source = float.source
            } else if session.described {
                guard let dropped = session.base.withLayerMeta(layer, nil) else {
                    throw DoctoredCoreError(message: "The layer's description could not be dropped.")
                }
                source = dropped
            } else {
                source = session.base
            }
            session.budget = try session.base.legacyPrecisionWorkContext()
            // Open-time validation only; each preview request plans itself.
            _ = try source.selectedPixelsPlan(request: float?.command(
                dx: session.start.x, dy: session.start.y).request
                ?? MoveSelectedPixels.command(
                    layer: layer, dx: 0, dy: 0, duplicate: session.duplicate,
                    bounds: captured.bounds, target: target,
                    background: session.background).request)
        } catch {
            document.presentError(error)
            return false
        }
        session.source = source
        let preview = TransformMaskPreview(
            source: source, revision: document.revision,
            image: initial ?? document.previewImage, display: display)
        preview.onUpdate = { [weak self, weak preview] in
            guard let self, let preview, self.moveSelectedSession?.preview === preview else { return }
            self.canvas.previewImage = preview.image
            self.canvas.needsDisplay = true
        }
        session.preview = preview
        canvas.precisionMoveActive = true
        return true
    }

    /// The float query's answer on the current, opened, unrefused session. A
    /// rect becomes the snap box (the float's opaque bounds, which ⌘T
    /// uses too; earlier ticks snapped on selection ∩ layer) and the drag
    /// goes on; nil (an EMPTY float) tears the preview down and refuses by
    /// name, so the release commits nothing.
    private func moveSelectedFloatAnswered(_ rect: CGRect?, query: SelectedPixelsFloatQuery) {
        guard var session = moveSelectedSession, session.floatQuery === query,
              session.opened, !session.refused, let document else { return }
        if let rect {
            // A continued float's query ran on its source, at the lift.
            movePressBox = rect.offsetBy(dx: CGFloat(session.start.x), dy: CGFloat(session.start.y))
            return
        }
        query.cancel()
        session.floatQuery = nil
        session.preview?.cancel()
        session.preview = nil
        if let gesture = precisionMoveGesture {
            precisionMoveGesture = nil
            gesture.cancel()
        }
        canvas.previewImage = nil
        canvas.precisionMoveActive = false
        canvas.moveSelectionOffset = nil
        canvas.needsDisplay = true
        session.refused = true
        moveSelectedSession = session
        document.presentError(DoctoredCoreError(message: MoveSelectedPixels.emptyRefusal))
    }

    /// The release: one "Move" step with the moved selection, or nothing for
    /// a click, a refusal or a drag that ended where it began.
    func finishMoveSelected() {
        guard let session = moveSelectedSession else { return }
        moveSelectedSession = nil
        session.floatQuery?.cancel()
        // The marquee keeps its moved position through the commit wait.
        defer {
            canvas.moveSelectionOffset = nil
            moveDidEnd()
        }
        let delta = session.applied
        let total = (x: session.start.x + delta.x, y: session.start.y + delta.y)
        let commit = session.opened && !session.refused && (delta.x != 0 || delta.y != 0)
        let name = session.duplicate
            ? MoveSelectedPixels.duplicateActionName : MoveSelectedPixels.actionName
        // The step records this drag's own delta and ⌥.
        let record = MoveSelectedPixels.record(session.base, layer: session.layer,
                                               dx: delta.x, dy: delta.y,
                                               duplicate: session.duplicate,
                                               target: session.target,
                                               background: session.background?.hex)
        // The selection moves with the pixels, or with the copy; a continued
        // float's lands at its total.
        let resulting = session.continuing.map {
            $0.float.resultingSelection(from: session.selection, dx: total.x, dy: total.y)
        } ?? session.selection.moved(dx: delta.x, dy: delta.y)
        if let preview = session.preview {
            defer {
                canvas.previewImage = nil
                canvas.precisionMoveActive = false
                canvas.needsDisplay = true
            }
            guard commit, let document, let source = session.source else {
                preview.cancel()
                return
            }
            // Cancel first; the preview stays on screen through the wait so
            // the canvas never flashes the unmoved pixels. Only a commit
            // waits for the preview's reservations to leave admission.
            preview.cancelReleasingReservations()
            if let continuing = session.continuing, total.x == 0, total.y == 0 {
                publishMoveFloatOrigin(document, continuing, base: session.base, name: name,
                                       record: record, resulting: resulting)
                return
            }
            do {
                let request = try moveSelectedCommand(session, dx: delta.x, dy: delta.y).request
                guard let prepared = try MoveSelectedPixels.prepared(
                    document, source: source, request: request, coverage: session.coverage,
                    title: name, base: session.base, selection: session.selection)
                else {
                    NSSound.beep()
                    return
                }
                document.preparedEdit = prepared
                guard document.admitsPublication(prepared.document, record: record) else { return }
                document.publishPrepared(prepared, name: name, record: record,
                                         resultingSelection: resulting)
                noteMoveSelected(document, session, source: source, total: total)
            } catch { document.presentError(error) }
            return
        }
        guard let gesture = precisionMoveGesture else { return }
        precisionMoveGesture = nil
        canvas.precisionMoveActive = false
        defer {
            gesture.cancel()
            canvas.previewImage = nil
        }
        guard commit, let document else { return }
        if let continuing = session.continuing, total.x == 0, total.y == 0 {
            publishMoveFloatOrigin(document, continuing, base: session.base, name: name,
                                   record: record, resulting: resulting)
            return
        }
        do {
            let command = try moveSelectedCommand(session, dx: delta.x, dy: delta.y)
            try gesture.finish(.moveSelected(command: command, coverage: session.coverage),
                               name: name, record: record, resultingSelection: resulting)
            noteMoveSelected(document, session, source: session.base, total: total)
        } catch { document.presentError(error) }
    }

    /// The drag's request at drag delta (dx, dy): a continued float's at its
    /// TOTAL, else the fresh lift's.
    private func moveSelectedCommand(_ session: MoveSelectedSession, dx: Int,
                                     dy: Int) throws -> PrecisionPixelCommand {
        if let float = session.continuing?.float {
            return try float.command(dx: session.start.x + dx, dy: session.start.y + dy)
        }
        return try MoveSelectedPixels.command(
            layer: session.layer, dx: dx, dy: dy, duplicate: session.duplicate,
            bounds: session.selection.selection?.bounds, target: session.target,
            background: session.background)
    }

    /// After a drag's publication, the float it continued advances, or
    /// its lift from `source` starts one.
    private func noteMoveSelected(_ document: ImageDocument, _ session: MoveSelectedSession,
                                  source: RasterDocument, total: (x: Int, y: Int)) {
        guard let bounds = session.selection.selection?.bounds else { return }
        document.noteMoveStep(base: session.base, continuing: session.continuing, lift: {
            MoveFloat(source: source, before: session.base, selection: session.selection,
                      layer: session.layer, target: session.target,
                      background: session.background, duplicate: session.duplicate,
                      coverage: session.coverage, bounds: bounds)
        }, dx: total.x, dy: total.y)
    }

    /// A continued float dragged or nudged back to its lift publishes
    /// its source itself with the lift selection, one step, at either depth.
    private func publishMoveFloatOrigin(_ document: ImageDocument,
                                        _ continuing: (float: MoveFloat, at: Int),
                                        base: RasterDocument, name: String,
                                        record: [ActionStep], resulting: SelectionHistoryState) {
        let source = continuing.float.source
        document.applyEdit(name, record: record, resultingSelection: resulting) {
            $0 === base ? source : nil
        }
        document.noteMoveStep(base: base, continuing: continuing, lift: { continuing.float },
                              dx: 0, dy: 0)
    }

    /// Escape or a tool switch: publishes nothing, at either depth.
    func cancelMoveSelected() {
        guard let session = moveSelectedSession else { return }
        moveSelectedSession = nil
        session.floatQuery?.cancel()
        session.preview?.cancel()
        if let gesture = precisionMoveGesture {
            precisionMoveGesture = nil
            gesture.cancel()
        }
        if session.opened {
            canvas.previewImage = nil
            canvas.precisionMoveActive = false
        }
        canvas.moveSelectionOffset = nil
        moveAppliedDelta = nil
        moveDidEnd()
    }

    /// An arrow-key nudge over a selection: the same refusals and prompt as
    /// the drag, one "Move" step per press. ⌥ (`duplicate`) is the
    /// drag's duplicate: no hole, one "Duplicate" step per press.
    func moveSelectedNudge(_ dx: Int, _ dy: Int, duplicate: Bool) {
        guard let document, let doc = document.doc,
              let selection = document.canvasSelection else { return }
        let layer = document.activeLayerIndex
        let history = document.selectionHistory
        let target = document.planeEditTarget
        if target != .layer {
            moveSelectedPlaneNudge(document, doc, layer: layer, target: target,
                                   selection: selection, dx: dx, dy: dy, duplicate: duplicate)
            return
        }
        // A plain nudge over a live float continues it; ⌥ stamps a fresh copy.
        let continuing = duplicate ? nil : document.liveMoveFloat(layer: layer, target: target)
        if refuseMoveSelected(document, layer: layer) { return }
        let described = continuing == nil && document.layerDescribesSource(layer)
        if described, !document.confirmRasterize(layer: layer) { return }
        guard document.doc === doc, document.selectionHistory == history else {
            document.presentError(DoctoredCoreError(message: MoveSelectedPixels.staleSentence))
            return
        }
        let coverage = continuing?.float.coverage ?? selection.maskSamples16()
        let record = MoveSelectedPixels.record(doc, layer: layer, dx: dx, dy: dy,
                                               duplicate: duplicate)
        let name = duplicate
            ? MoveSelectedPixels.duplicateActionName : MoveSelectedPixels.actionName
        let total = Self.moveFloatTotal(continuing, dx: dx, dy: dy)
        let resulting = continuing.map {
            $0.float.resultingSelection(from: history, dx: total.x, dy: total.y)
        } ?? history.moved(dx: dx, dy: dy)
        if let continuing, total.x == 0, total.y == 0 {
            publishMoveFloatOrigin(document, continuing, base: doc, name: name, record: record,
                                   resulting: resulting)
            return
        }
        let bounds = selection.bounds
        let lift = { (source: RasterDocument) in
            MoveFloat(source: source, before: doc, selection: history, layer: layer,
                      target: target, background: nil, duplicate: duplicate,
                      coverage: coverage, bounds: bounds)
        }
        do {
            let command = try continuing?.float.command(dx: total.x, dy: total.y)
                ?? MoveSelectedPixels.command(
                    layer: layer, dx: dx, dy: dy, duplicate: duplicate, bounds: bounds)
            if doc.isHighDepth {
                let gesture = try PrecisionGeometryGesture(
                    document, workSource: continuing?.float.source,
                    indexedBackground: backgroundColor)
                defer { gesture.cancel() }
                try gesture.finish(.moveSelected(command: command, coverage: coverage),
                                   name: name, record: record,
                                   resultingSelection: resulting)
                document.noteMoveStep(base: doc, continuing: continuing, lift: { lift(doc) },
                                      dx: total.x, dy: total.y)
                return
            }
            var source = continuing?.float.source ?? doc
            if described {
                guard let dropped = doc.withLayerMeta(layer, nil) else {
                    throw DoctoredCoreError(message: "The layer's description could not be dropped.")
                }
                source = dropped
            }
            let lifted = source
            guard let prepared = try MoveSelectedPixels.prepared(
                document, source: source, request: command.request, coverage: coverage,
                title: name, base: doc, selection: history)
            else {
                NSSound.beep()
                return
            }
            document.preparedEdit = prepared
            guard document.admitsPublication(prepared.document, record: record) else { return }
            document.publishPrepared(prepared, name: name, record: record,
                                     resultingSelection: resulting)
            document.noteMoveStep(base: doc, continuing: continuing, lift: { lift(lifted) },
                                  dx: total.x, dy: total.y)
        } catch { document.presentError(error) }
    }

    /// A nudge's TOTAL from a continued float's lift (the position it
    /// continues plus the press), or the press itself for a fresh lift.
    private static func moveFloatTotal(_ continuing: (float: MoveFloat, at: Int)?, dx: Int,
                               dy: Int) -> (x: Int, y: Int) {
        guard let continuing else { return (dx, dy) }
        let position = continuing.float.positions[continuing.at]
        return (position.dx + dx, position.dy + dy)
    }

    /// A nudge of a mask's or channel's selected area — the drag's
    /// plane refusal, box and background, no prompt; one step per press.
    /// A colour plane takes the drag's lock alert and rasterize prompt.
    private func moveSelectedPlaneNudge(_ document: ImageDocument, _ doc: RasterDocument,
                                        layer: Int, target: PaintTarget,
                                        selection: CanvasSelection, dx: Int, dy: Int,
                                        duplicate: Bool) {
        let history = document.selectionHistory
        // A plain nudge over a live float continues it; ⌥ stamps a fresh copy.
        let continuing = duplicate ? nil : document.liveMoveFloat(layer: layer, target: target)
        if let kind = MoveSelectedPixels.planeLockKind(target),
           refuseLockedEdit(layer: layer, kind: kind) { return }
        if let reason = MoveSelectedPixels.planeRefusal(
            document, layer: layer, target: target, verb: "move",
            subject: "Moving selected pixels") {
            document.presentError(DoctoredCoreError(message: reason))
            return
        }
        guard TransformSelectedPixels.planeBox(selection) != nil else {
            document.presentError(DoctoredCoreError(message: MoveSelectedPixels.emptyRefusal))
            return
        }
        let described = continuing == nil && target.isPlane && document.layerDescribesSource(layer)
        if described, !document.confirmRasterize(layer: layer) { return }
        guard document.doc === doc, document.selectionHistory == history else {
            document.presentError(DoctoredCoreError(message: MoveSelectedPixels.staleSentence))
            return
        }
        // A continued float keeps the background latched at its lift.
        let hex = continuing?.float.background?.hex ?? ActionArgs.hex(backgroundColor)
        let name = duplicate
            ? MoveSelectedPixels.duplicateActionName : MoveSelectedPixels.actionName
        let record = MoveSelectedPixels.record(doc, layer: layer, dx: dx, dy: dy,
                                               duplicate: duplicate, target: target,
                                               background: hex)
        let total = Self.moveFloatTotal(continuing, dx: dx, dy: dy)
        let resulting = continuing.map {
            $0.float.resultingSelection(from: history, dx: total.x, dy: total.y)
        } ?? history.moved(dx: dx, dy: dy)
        if let continuing, total.x == 0, total.y == 0 {
            publishMoveFloatOrigin(document, continuing, base: doc, name: name, record: record,
                                   resulting: resulting)
            return
        }
        let coverage = continuing?.float.coverage ?? selection.maskSamples16()
        let bounds = selection.bounds
        do {
            let background: PlaneBackground?
            if let continuing {
                background = continuing.float.background
            } else {
                background = try TransformSelectedPixels.planeBackground(
                    hex: hex, target: target, in: document)
            }
            let command = try continuing?.float.command(dx: total.x, dy: total.y)
                ?? MoveSelectedPixels.command(
                    layer: layer, dx: dx, dy: dy, duplicate: duplicate, bounds: bounds,
                    target: target, background: background)
            let lift = { (source: RasterDocument) in
                MoveFloat(source: source, before: doc, selection: history, layer: layer,
                          target: target, background: background, duplicate: duplicate,
                          coverage: coverage, bounds: bounds)
            }
            if doc.isHighDepth {
                let gesture = try PrecisionGeometryGesture(
                    document, workSource: continuing?.float.source,
                    indexedBackground: backgroundColor)
                defer { gesture.cancel() }
                try gesture.finish(.moveSelected(command: command, coverage: coverage),
                                   name: name, record: record,
                                   resultingSelection: resulting)
                document.noteMoveStep(base: doc, continuing: continuing, lift: { lift(doc) },
                                      dx: total.x, dy: total.y)
                return
            }
            var source = continuing?.float.source ?? doc
            if described {
                guard let dropped = doc.withLayerMeta(layer, nil) else {
                    throw DoctoredCoreError(message: "The layer's description could not be dropped.")
                }
                source = dropped
            }
            let lifted = source
            guard let prepared = try MoveSelectedPixels.prepared(
                document, source: source, request: command.request, coverage: coverage,
                title: name, base: doc, selection: history)
            else {
                NSSound.beep()
                return
            }
            document.preparedEdit = prepared
            guard document.admitsPublication(prepared.document, record: record) else { return }
            document.publishPrepared(prepared, name: name, record: record,
                                     resultingSelection: resulting)
            document.noteMoveStep(base: doc, continuing: continuing, lift: { lift(lifted) },
                                  dx: total.x, dy: total.y)
        } catch { document.presentError(error) }
    }

    /// The Pixels lock's owner alert, then the named refusals.
    /// Presents; true = refused.
    func refuseMoveSelected(_ document: ImageDocument, layer: Int) -> Bool {
        if refuseLockedEdit(layer: layer, kind: DR_EDIT_PIXELS) { return true }
        guard let reason = MoveSelectedPixels.refusal(
            document, layer: layer, target: document.planeEditTarget) else { return false }
        document.presentError(DoctoredCoreError(message: reason))
        return true
    }
}
