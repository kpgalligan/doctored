import AppKit

// The Straighten line (Draw line checkbox) redraws only `canvas.cropOverlay` per tick
// (`pushCropOverlay(refreshPreview: false)`), because each native preview request cancels and
// re-renders.

// The Crop tool (C): an interactive box over the canvas with aspect
// presets, a thirds grid and straighten, committed with Return or a
// double-click. The box starts as the whole canvas; the commit is
// dr_doc_crop — which only moves the canvas window, so a plain crop is
// non-destructive — preceded, when straightening, by the same per-layer
// affine rotate Free Transform uses, about the box's center, applied to
// every layer and to every alpha channel. With Delete Cropped Pixels
// each plain raster is then trimmed to the new canvas
// (dr_doc_delete_cropped_pixels; the typed straighten's `delete_cropped`).
//
// Session geometry lives in CropTool.swift; the canvas draws the overlay
// and routes the gesture here. Mirrored for the agent by the `crop` MCP
// tool (rect today, `angle` for straighten).
//
// Phase 12F-1b2: the tool is admitted at 16 bits as well. The box and the
// rotated preview are display-only at every depth, and the commit is the
// ONE host composition `RasterDocument.straightenedCrop` (CropTool.swift),
// whose typed branch is the core's single `straighten` op. Angle 0 keeps its
// own route through `cropped` below, at BOTH depths.
extension EditorViewController {
    /// Aspect presets, in the ratio popup's order. nil aspect = free;
    /// "Original" resolves to the document's own aspect at use time.
    static let cropRatios: [(title: String, aspect: Double?)] = [
        ("Original", nil), ("Free", nil), ("1 : 1", 1),
        ("4 : 3", 4.0 / 3), ("3 : 2", 1.5), ("16 : 9", 16.0 / 9),
    ]

    /// Entering the crop tool: the box opens over the whole canvas.
    func beginCropSession() {
        guard let doc = document?.doc else { return }
        cropSession?.precisionGesture?.cancel()
        var session = CropSession(
            canvas: CGSize(width: CGFloat(doc.width), height: CGFloat(doc.height)))
        session.ratio = currentCropAspect()
        if doc.isHighDepth, let document {
            do {
                session.precisionGesture = try PrecisionGeometryGesture(
                    document, indexedBackground: backgroundColor)
            }
            catch { document.presentError(error); return }
        }
        cropSession = session
        pushCropOverlay()
    }

    /// Leaving the tool (or a canceled commit): the box just goes away —
    /// the document was never touched.
    func endCropSession() {
        cropSession?.precisionGesture?.cancel()
        cropSession = nil
        canvas.cropOverlay = nil
        canvas.precisionCropPreview = nil
        canvas.precisionCropWindow = nil
    }

    /// Escape: back to the full canvas, angle zeroed.
    func resetCropSession() {
        guard cropSession != nil else { return }
        if cropSession?.precisionGesture != nil {
            endCropSession()
        }
        beginCropSession()
        optionsBar.refreshValues()
    }

    /// The active ratio preset as an aspect (width / height); "Original"
    /// reads the document, "Free" is nil.
    private func currentCropAspect() -> Double? {
        let index = min(max(ToolOptionsStore.shared.crop.ratioIndex, 0),
                        Self.cropRatios.count - 1)
        if index == 0, let doc = document?.doc, doc.height > 0 {
            return Double(doc.width) / Double(doc.height)
        }
        return Self.cropRatios[index].aspect
    }

    /// `refreshPreview` false redraws the overlay only: a Straighten line
    /// tick changes neither the box nor the angle, and every native preview
    /// request cancels and re-renders, so asking per tick would keep the
    /// 16-bit/float preview pending for the whole drag.
    private func pushCropOverlay(refreshPreview: Bool = true) {
        guard let session = cropSession else {
            canvas.cropOverlay = nil
            return
        }
        canvas.cropOverlay = CropOverlay(
            rect: session.rect,
            angle: CGFloat(session.angle),
            showsThirds: ToolOptionsStore.shared.crop.gridIndex == 1,
            straightenLine: session.straightenLine)
        if refreshPreview, session.precisionGesture != nil { refreshPrecisionCropPreview(session) }
    }

    /// The canvas size the box fits against; nil without a document.
    private var cropCanvasSize: CGSize? {
        guard let doc = document?.doc else { return nil }
        return CGSize(width: CGFloat(doc.width), height: CGFloat(doc.height))
    }

    /// The options bar's Draw line checkbox.
    func setCropStraightenLineMode(_ on: Bool) {
        guard cropSession != nil else { return }
        cropSession?.drawsStraightenLine = on
        cropSession?.straightenLine = nil
        pushCropOverlay(refreshPreview: false)
        optionsBar.refreshValues()
    }

    // MARK: - Gesture

    func cropMouseDown(_ point: CGPoint) {
        guard var session = cropSession else { return }
        if session.drawsStraightenLine {
            // Straighten line mode: the press starts a line, never a drag.
            session.drag = nil
            session.straightenLine = (point, point)
            cropSession = session
            pushCropOverlay(refreshPreview: false)
            return
        }
        let slop = ImageCanvasView.transformHandleSize / canvas.magnification
        if let handle = CropSession.handleIndex(at: point, rect: session.rect, slop: slop) {
            session.drag = .handle(handle, start: session.rect)
        } else if session.rect.contains(point) {
            session.drag = .move(grab: point, start: session.rect)
        } else {
            session.drag = .draw(anchor: point)
        }
        cropSession = session
    }

    /// `modifiers` carries ⌃, which suspends snapping for this tick — read
    /// per tick, never latched, so it can be pressed and released mid-drag.
    ///
    /// The snap goes on the POINT, ahead of `resizing` and `bounded`, for
    /// `.handle` and `.draw` — so that chain runs unchanged and
    /// "snap → re-derive → bound" falls out for free. `.move`'s point is a
    /// grab OFFSET, so it takes the delta form instead
    /// (`DragSnapping.snapCropPoint` / `snapCropDelta`).
    ///
    /// Every box edit writes `requested` and then fits it inside the rotated
    /// image (`CropFit`): a handle about the opposite corner or edge, a
    /// rubber band about its anchor corner, a move by sliding the box.
    func cropMouseDragged(_ point: CGPoint, _ modifiers: NSEvent.ModifierFlags) {
        guard var session = cropSession else { return }
        if let line = session.straightenLine {
            // Unsnapped: the line follows the pointer. Overlay only.
            session.straightenLine = (line.start, point)
            cropSession = session
            pushCropOverlay(refreshPreview: false)
            return
        }
        guard let drag = session.drag, let doc = document?.doc else { return }
        let canvasSize = CGSize(width: CGFloat(doc.width), height: CGFloat(doc.height))
        switch drag {
        case let .handle(index, start):
            // The POINT, before `resizing` — never `session.rect` after it.
            // `resizing` takes a point plus a handle index and cannot be
            // re-run from a rect, so a rect snapped here would have already
            // been through the ratio derivation and the canvas clamp, and
            // snapping it would break the very aspect ratio the ordering
            // rule protects.
            let snapped = snapCropPoint(point, drag: drag, modifiers: modifiers)
            session.requested = CropSession.bounded(
                CropSession.resizing(start, handle: index, to: snapped, ratio: session.ratio),
                ratio: session.ratio)
            session.refit(canvas: canvasSize,
                          anchor: CropFit.resizeAnchor(of: session.requested, handle: index))
        case let .move(grab, start):
            // The point here is a grab OFFSET inside the box, not the moved
            // geometry, so what snaps is the DELTA against the press-time
            // box. A box moved past the canvas is a crop past it; one
            // inside keeps `movedInsideRotation`.
            let delta = snapCropDelta(
                from: start, by: CGVector(dx: point.x - grab.x, dy: point.y - grab.y),
                modifiers: modifiers)
            let moved = start.offsetBy(dx: delta.dx, dy: delta.dy)
            session.rect = CropSession.extends(
                CropSession.roundedWindow(moved, canvas: canvasSize), canvas: canvasSize)
                ? moved
                : CropFit.movedInsideRotation(moved, angle: session.angle, canvas: canvasSize)
            session.requested = session.rect
        case let .draw(anchor):
            // BOTH corners snap, and the fixed one is snapped on every tick
            // rather than at mouse-down: `onCropMouseDown` carries no
            // modifier flags, so an anchor snapped there could not be
            // suspended with ⌃ — and the anchor does not move, so its own
            // pull is the same answer on every tick.
            let corner = snapCropPoint(anchor, drag: drag, modifiers: modifiers)
            let moved = snapCropPoint(point, drag: drag, modifiers: modifiers)
            var rect = CGRect(
                x: min(corner.x, moved.x), y: min(corner.y, moved.y),
                width: max(abs(moved.x - corner.x), 1), height: max(abs(moved.y - corner.y), 1))
            if let ratio = session.ratio, ratio > 0 {
                let height = rect.width / CGFloat(ratio)
                // The re-derived height grows away from the ANCHOR: an
                // upward drag keeps the anchor as the bottom edge.
                if moved.y < corner.y {
                    rect.origin.y = corner.y - height
                }
                rect.size.height = height
            }
            session.requested = CropSession.bounded(rect, ratio: session.ratio)
            session.refit(canvas: canvasSize, anchor: corner)
        }
        cropSession = session
        pushCropOverlay()
        optionsBar.refreshValues()
    }

    /// A Straighten line at least 4 screen points long sets the angle that
    /// levels it — added to the current angle, because the line was drawn
    /// over the already-rotated preview — and turns line mode off. A
    /// shorter one changes nothing and keeps the mode on.
    func cropMouseUp(_ point: CGPoint) {
        cropSession?.drag = nil
        guard let line = cropSession?.straightenLine else { return }
        cropSession?.straightenLine = nil
        let end = point
        let length = hypot(end.x - line.start.x, end.y - line.start.y) * canvas.magnification
        if length >= 4, let delta = CropFit.lineAngle(from: line.start, to: end) {
            // The setter fits the box and requests the native preview.
            cropStraightenDegrees = cropStraightenDegrees + delta
            cropSession?.drawsStraightenLine = false
        }
        pushCropOverlay(refreshPreview: false)
        optionsBar.refreshValues()
    }

    // MARK: - Commit

    /// Return / double-click: straighten (a rotate about the box's center,
    /// over every layer and every alpha channel) then crop, as ONE undo
    /// step. A full-canvas box at angle 0 is a no-op and just keeps the
    /// session.
    func commitCropSession() {
        guard let session = cropSession, let document = document, let doc = document.doc
        else { return }
        if session.precisionGesture != nil {
            commitPrecisionCropSession(session)
            return
        }
        // Round each edge rather than .integral (which rounds OUTWARD and
        // could commit one pixel more than the W/H fields displayed).
        let canvasSize = CGSize(width: doc.width, height: doc.height)
        let rect = CropSession.roundedWindow(session.rect, canvas: canvasSize)
        guard rect.width >= 1, rect.height >= 1 else {
            NSSound.beep()
            return
        }
        // A window past the canvas extends it, with the Background
        // (Default) fill; its size is refused first by Canvas Size's own
        // sentences.
        let extended = CropSession.extends(rect, canvas: canvasSize)
        var background: CropBackground?
        if extended {
            if let refusal = doc.extendedCropRefusal(width: Int(rect.width),
                                                     height: Int(rect.height)) {
                document.presentError(DoctoredCoreError(message: refusal))
                return
            }
            do {
                background = try CropBackground.make(
                    backgroundColor, cmykInk: ToolOptionsStore.shared.sharedState.backgroundInk,
                    labInk: ToolOptionsStore.shared.sharedState.backgroundLab, in: doc)
            } catch {
                document.presentError(error)
                return
            }
        }
        let angle = session.angle
        let fullCanvas = rect == CGRect(x: 0, y: 0, width: doc.width, height: doc.height)
        guard !fullCanvas || angle != 0 else { return }

        // Straightening resamples every layer, which rewrites the pixels a
        // described layer was rendered from. Text and shape layers, the only
        // described kinds, are regenerated from their descriptions inside the
        // same edit (`straightenedCrop`), so nothing is asked. A plain crop
        // only moves the canvas window and keeps every description valid.
        var describedLayers: [Int] = []
        if angle != 0 {
            describedLayers = (0..<doc.layerCount).filter { document.layerDescribesSource($0) }
        }

        // The straighten resamples with the editor's shared Free Transform
        // sampler, so the recorded step carries it: `crop` takes the same
        // sampler vocabulary, and without it a replay of a Nearest straighten
        // came back smoothed with nothing in the step saying so.
        let sampler = transformSampler
        let deleteCropped = ToolOptionsStore.shared.crop.deleteCroppedPixels
        // A Fill other than Background (Default) runs its own job on the crop
        // (EditorViewController+CropFill.swift); Content-Aware deletes the cropped pixels.
        let fill = CropFill(index: ToolOptionsStore.shared.crop.fillIndex)
        if extended, fill != .background, let background {
            commitCropFill(CropFillRequest(
                rect: rect, angle: angle, sampler: sampler,
                deleteCropped: deleteCropped || fill == .contentAware, background: background,
                described: describedLayers), fill: fill)
            return
        }
        let before = document.doc
        // A core refusal arrives as a thrown sentence and propagates to
        // `applyEdit`, which PRESENTS it (no beep): the core's sentence is
        // the only thing that can tell the user what to do about it. The
        // straighten runs as ONE background job with its display preparation,
        // so Cancel/Esc throws `JobCancelled`, which `applyEdit`
        // presents as nothing, and the box stays (the guard on `before`
        // below). The agent's `crop` handler captures the same error and
        // rethrows it as the ToolError.
        defer { document.preparedEdit = nil }
        // nil without a throw from the straighten: the WHOLE canvas is
        // unchanged, said instead of `applyEdit`'s generic refusal.
        var unchanged = false
        document.applyEdit(
            "Crop",
            record: .crop(rect, angle: angle, sampler: Self.samplerAgentName(sampler),
                          deleteCropped: deleteCropped,
                          backgroundColor: extended ? backgroundColor : nil),
            changesCanvasGeometry: true,
            // The selection goes through the crop's own map.
            resultingSelection: document.selectionCarried(
                by: CropSession.canvasMap(window: rect, angle: angle),
                width: Int(rect.width), height: Int(rect.height))
        ) { doc in
            let current: RasterDocument = doc
            if angle != 0 {
                // The preview rotated the image by −angle about the box
                // center; the commit is the identical matrix. ONE host
                // composition, shared with the agent's `crop` handler
                // (`RasterDocument.straightenedCrop`, CropTool.swift): one
                // job at both depths, whose prepared display `applyEdit`
                // publishes without preparing it again.
                //
                // Angle 0 deliberately does NOT come here and falls through to
                // `cropped` below at BOTH depths — a plain crop needs no
                // resample, and at 16 bits the typed op would bypass
                // `cropped`'s artwork-description capture/restore.
                let center = CGPoint(x: rect.midX, y: rect.midY)
                let matrix = CGAffineTransform(translationX: center.x, y: center.y)
                    .rotated(by: CGFloat(-angle) * .pi / 180)
                    .translatedBy(x: -center.x, y: -center.y)
                guard let prepared = try current.straightenedCrop(
                    matrix, sampler: sampler,
                    x: Int(rect.minX), y: Int(rect.minY),
                    w: Int(rect.width), h: Int(rect.height),
                    described: describedLayers, deleteCropped: deleteCropped,
                    background: background) else {
                    unchanged = true
                    return nil
                }
                document.preparedEdit = prepared
                return prepared.document
            }
            if extended {
                return try current.extendedCrop(
                    x: Int(rect.minX), y: Int(rect.minY),
                    w: Int(rect.width), h: Int(rect.height), deleteCropped: deleteCropped,
                    background: background)
            }
            return try current.cropped(
                x: Int(rect.minX), y: Int(rect.minY),
                w: Int(rect.width), h: Int(rect.height), deleteCropped: deleteCropped)
        }
        if unchanged {
            // `applyEdit` already beeped; the last note wins, so no second beep.
            SessionCommitRefusal.note(CropSession.unchangedStraightenSentence(
                width: doc.width, height: doc.height, angle: angle))
        }
        guard document.doc !== before else { return }
        // The canvas is a new size; reopen the box over all of it.
        beginCropSession()
        optionsBar.refreshValues()
    }

    // MARK: - Options bar bindings

    var cropRectWidth: Double {
        get { Double(cropSession?.rect.width ?? 0) }
        set {
            guard var session = cropSession, let size = cropCanvasSize, newValue >= 1
            else { return }
            var rect = session.rect
            rect.size.width = CGFloat(newValue)
            if let ratio = session.ratio, ratio > 0 {
                rect.size.height = rect.width / CGFloat(ratio)
            }
            session.requested = CropSession.bounded(rect, ratio: session.ratio)
            session.refit(canvas: size)
            cropSession = session
            pushCropOverlay()
            optionsBar.refreshValues()
        }
    }

    var cropRectHeight: Double {
        get { Double(cropSession?.rect.height ?? 0) }
        set {
            guard var session = cropSession, let size = cropCanvasSize, newValue >= 1
            else { return }
            var rect = session.rect
            rect.size.height = CGFloat(newValue)
            if let ratio = session.ratio, ratio > 0 {
                rect.size.width = rect.height * CGFloat(ratio)
            }
            session.requested = CropSession.bounded(rect, ratio: session.ratio)
            session.refit(canvas: size)
            cropSession = session
            pushCropOverlay()
            optionsBar.refreshValues()
        }
    }

    /// The angle re-fits the box from `requested` (never from the fitted
    /// `rect`), so stepping the angle back returns the same box.
    var cropStraightenDegrees: Double {
        get { cropSession?.angle ?? 0 }
        set {
            guard var session = cropSession, let size = cropCanvasSize else { return }
            session.angle = min(max(newValue, -45), 45)
            session.refit(canvas: size)
            cropSession = session
            pushCropOverlay()
            optionsBar.refreshValues()
        }
    }

    /// The ratio popup changed: re-constrain the current box to it.
    /// "Free" keeps the box as it is, aspect included.
    func cropRatioChanged() {
        guard var session = cropSession, let size = cropCanvasSize else { return }
        session.ratio = currentCropAspect()
        if let ratio = session.ratio, ratio > 0 {
            var rect = session.rect
            rect.size.height = rect.width / CGFloat(ratio)
            session.requested = CropSession.bounded(rect, ratio: session.ratio)
            session.refit(canvas: size)
        }
        cropSession = session
        pushCropOverlay()
        optionsBar.refreshValues()
    }

    /// The grid popup changed: redraw the overlay with/without thirds.
    func cropGridChanged() {
        pushCropOverlay()
    }
}
