import AppKit

// Retouching and Stroke Path in a 32-bit float document, through the
// canvas's own `beginStroke(at:)` / `continueStroke(to:pressure:)` /
// `endStroke()`, its `patchSession`, and the controller's commits, so the
// case needs no key window. The Clone Stamp copies linear light (a source
// above white lands as its own numbers); the healing tools each run as one
// job and one undo step; Stroke Path with a foreground above white records
// `rgba32`.

private let retouchInk: [Double] = [4, 0.5, 0.25, 1]
private let retouchSize = (width: 160, height: 120)

/// One float retouch stroke through the canvas: press, one drag, release.
private func retouchStroke(_ controller: EditorViewController, from: CGPoint, to: CGPoint) throws {
    let canvas: ImageCanvasView = controller.canvas
    canvas.beginStroke(at: from)
    try require(canvas.strokeActive, "[float retouch] the \(canvas.tool) stroke began")
    canvas.continueStroke(to: to, pressure: nil)
    canvas.endStroke()
}

/// The job the last gesture started, pumped to its end.
private func finishedRetouchJob(_ what: String) throws -> DocumentJob {
    guard let job = DocumentJobs.active else {
        throw ProbeFailure(description: "\(what) started no job")
    }
    try pumpUntil({ job.state != "running" })
    return job
}

/// The composite's straight linear RGBA at one pixel.
private func retouchPixel(_ document: ImageDocument, _ x: Int, _ y: Int) throws -> [Double] {
    guard let sample = try FloatReadout.at((x: x, y: y), document: document) else {
        throw ProbeFailure(description: "No float sample at (\(x), \(y))")
    }
    return sample.values
}

private func retouchNear(_ got: [Double], _ want: [Double], _ tolerance: Double = 1e-4) -> Bool {
    got.count == want.count
        && zip(got, want).allSatisfy { abs($0 - $1) <= tolerance * max(1, abs($1)) }
}

/// One Patch-session drag through the canvas: press inside `region`, drag by
/// `offset`, release, and hand the result to `onPatchCommit` (the Patch tool
/// and Content-Aware Move share the session).
private func retouchDrag(_ controller: EditorViewController, _ region: CanvasSelection,
                         offset: CGVector) throws {
    let canvas: ImageCanvasView = controller.canvas
    let from = CGPoint(x: region.bounds.midX, y: region.bounds.midY)
    let to = CGPoint(x: from.x + offset.dx, y: from.y + offset.dy)
    _ = canvas.patchSession.mouseDown(from, clickCount: 1, selection: region, in: canvas.image,
        magnification: 1, sampling: { controller.patchCapture() })
    _ = canvas.patchSession.mouseDragged(to)
    guard let result = canvas.patchSession.mouseUp(to) else {
        throw ProbeFailure(description: "[float retouch] the drag produced no commit")
    }
    try require(result.capture != nil, "[float retouch] the drag's capture was taken")
    canvas.onPatchCommit?(result)
}

/// The retouch stores for one case, restored afterwards.
private func withFloatRetouchOptions(_ body: () throws -> Void) throws {
    let store = ToolOptionsStore.shared
    let saved = (store.brush, store.clone, store.heal, store.spotHeal, store.remove, store.patch,
                 store.contentAwareMove, store.removeTool)
    defer {
        (store.brush, store.clone, store.heal, store.spotHeal, store.remove, store.patch,
         store.contentAwareMove, store.removeTool) = saved
    }
    // Case (f) asserts the Remove stroke's Content-Aware Fill job, one stroke at a time.
    var removeTool = RemoveToolOptions()
    removeTool.mode = .contentAware
    removeTool.removeAfterEachStroke = true
    store.removeTool = removeTool
    var tip = PaintToolOptions()
    tip.smoothing = 0
    tip.pressureSize = false
    tip.aligned = false
    tip.sample = .current
    store.brush = tip
    store.clone = tip
    store.heal = tip
    store.spotHeal = tip
    store.remove = tip
    var patch = store.patch
    patch.directionIndex = 0
    patch.sample = .current
    patch.ignoreAdjustments = false
    store.patch = patch
    var move = store.contentAwareMove
    move.modeIndex = 0
    move.sample = .current
    move.ignoreAdjustments = false
    store.contentAwareMove = move
    try body()
}

func verifyFloatRetouchStrokes() throws {
    let document = try floatFixture()
    defer {
        if ActionRecorder.shared.isRecording { _ = ActionRecorder.shared.stop() }
        document.close()
    }
    let controller = try editor(document)
    let canvas: ImageCanvasView = controller.canvas
    let (width, height) = retouchSize
    let colors = (controller.paintColor, controller.backgroundColor)
    defer { (controller.paintColor, controller.backgroundColor) = colors }

    // The picture: 160 × 120, red a ramp from 0 to 8 across the width (above
    // white from column 20 on), the same in every row, with one dark blob.
    _ = try agentOK("canvas_size", document,
                    ["width": width, "height": height, "anchor": "top-left"])
    _ = try agentOK("gradient", document, [
        "layer": 0, "start_rgba32": [0, 0.5, 0.25, 1], "end_rgba32": [8, 0.5, 0.25, 1],
        "x0": 0.5, "y0": 60, "x1": Double(width) - 0.5, "y1": 60,
    ])
    let blobCentre = (x: 102, y: 80)
    _ = try agentOK("brush_stroke", document, [
        "layer": 0, "points": [[100.5, 80.5], [104.5, 80.5]], "size": 10, "hardness": 100,
        "rgba32": [0.01, 0.01, 0.01, 1],
    ])
    document.undoManager?.removeAllActions()
    guard let base = document.doc, base.isFloat, base.width == width, base.height == height else {
        throw ProbeFailure(description: "[float retouch] the 160 × 120 float picture was not built")
    }
    let scene = { (x: Int) -> [Double] in [8 * Double(x) / Double(width - 1), 0.5, 0.25, 1] }
    let ramped = try retouchPixel(document, 150, 100)
    try require(retouchNear(ramped, scene(150)),
                "[float retouch] the ramp fills the grown layer: \(ramped)")
    let dark = try retouchPixel(document, blobCentre.x, blobCentre.y)
    try require(retouchNear(dark, [0.01, 0.01, 0.01, 1]),
                "[float retouch] the blob is painted: \(dark)")
    guard let hdr = ExtendedRangeColor.linearColor(retouchInk, in: base) else {
        throw ProbeFailure(description: "[float retouch] the above-white foreground did not build")
    }

    // The tool rail: the six retouch tools, Red Eye, the Blur, Sharpen, Smudge
    // and History brushes, and the bucket, Dodge, the Sponge and the Magic Wand
    // are all admitted.
    for tool in [EditorTool.clone, .heal, .spotHeal, .patch, .remove, .contentAwareMove, .redEye,
                 .blurBrush, .sharpenBrush, .smudge, .historyBrush, .fill, .dodge, .sponge,
                 .wand] {
        try require(DocumentPrecision.allows(tool, document: document),
                    "[float retouch] \(tool) is admitted")
    }

    try withFloatRetouchOptions {
        let from = CGPoint(x: 30.5, y: 40.5), to = CGPoint(x: 50.5, y: 40.5)
        let source = CGPoint(x: 120.5, y: 40.5)
        let centre = (x: 40, y: 40)

        // (a) A Clone Stamp stroke: the source overlay and the stroke's own
        // source image display with no alert, one job lands the source's
        // above-white values, one undo.
        controller.selectTool(.clone)
        try require(controller.currentTool == .clone, "[float retouch] the Clone Stamp is selected")
        canvas.brushSize = 10
        canvas.brushOpacity = 1
        canvas.cloneSource = source
        AutomationAlerts.reset()
        try require(controller.retouchSourceOverlayImage() != nil,
                    "[float retouch] the source overlay displays a float plate")
        let wanted = try retouchPixel(document, centre.x + 90, centre.y)
        try require(wanted[0] > 1, "[float retouch] the clone's source is above white: \(wanted)")
        canvas.beginStroke(at: from)
        try require(canvas.strokeActive, "[float retouch] the Clone Stamp stroke began")
        guard let geometry = canvas.strokeCloneGeometry else {
            throw ProbeFailure(description: "[float retouch] the Clone Stamp stroke has no source geometry")
        }
        try require(controller.strokeSourceImage(geometry) != nil,
                    "[float retouch] the stroke's source image displays a float plate")
        canvas.continueStroke(to: to, pressure: nil)
        canvas.endStroke()
        try require(AutomationAlerts.recorded.isEmpty,
                    "[float retouch] no alert while previewing: \(AutomationAlerts.recorded)")
        let clone = try finishedRetouchJob("[float retouch] the Clone Stamp stroke")
        try require(clone.state == "completed" && document.doc !== base,
                    "[float retouch] the Clone Stamp stroke completed: \(clone.report)")
        let cloned = try retouchPixel(document, centre.x, centre.y)
        try require(retouchNear(cloned, wanted),
                    "[float retouch] the stroke's centre reads the source above white: "
                    + "\(cloned) vs \(wanted)")
        document.undoManager?.undo()
        try require(document.doc === base && document.undoManager?.canUndo != true,
                    "[float retouch] the Clone Stamp stroke is exactly one undo")

        // (b) A clone cancelled before the first poll applies nothing.
        try retouchStroke(controller, from: from, to: to)
        guard let cancelled = DocumentJobs.active else {
            throw ProbeFailure(description: "[float retouch] the cancel case started no job")
        }
        cancelled.control.cancel()
        try pumpUntil({ cancelled.state != "running" })
        try require(cancelled.state == "cancelled" && document.doc === base
                    && document.undoManager?.canUndo != true,
                    "[float retouch] a cancelled clone applies nothing: \(cancelled.report)")

        // (c) The Healing Brush from 40 rows up, over the blob: one job.
        let healFrom = CGPoint(x: 96.5, y: 80.5), healTo = CGPoint(x: 108.5, y: 80.5)
        controller.selectTool(.heal)
        canvas.brushSize = 24
        canvas.cloneSource = CGPoint(x: healFrom.x, y: healFrom.y - 40)
        try retouchStroke(controller, from: healFrom, to: healTo)
        let heal = try finishedRetouchJob("[float retouch] the Healing Brush stroke")
        try require(heal.state == "completed" && document.doc !== base,
                    "[float retouch] the Healing Brush stroke completed: \(heal.report)")
        let healed = try retouchPixel(document, blobCentre.x, blobCentre.y)
        try require(retouchNear(healed, scene(blobCentre.x), 0.02),
                    "[float retouch] the heal restores the scene above white: \(healed)")
        document.undoManager?.undo()
        try require(document.doc === base && document.undoManager?.canUndo != true,
                    "[float retouch] the Healing Brush stroke is exactly one undo")

        // (d) Spot Healing over the blob: one job.
        controller.selectTool(.spotHeal)
        canvas.brushSize = 24
        try retouchStroke(controller, from: healFrom, to: healTo)
        let spot = try finishedRetouchJob("[float retouch] the Spot Healing stroke")
        try require(spot.state == "completed" && document.doc !== base,
                    "[float retouch] the Spot Healing stroke completed: \(spot.report)")
        let spotted = try retouchPixel(document, blobCentre.x, blobCentre.y)
        try require(retouchNear(spotted, scene(blobCentre.x), 0.05),
                    "[float retouch] the spot heal restores the scene: \(spotted)")
        document.undoManager?.undo()
        try require(document.doc === base && document.undoManager?.canUndo != true,
                    "[float retouch] the Spot Healing stroke is exactly one undo")

        // (e) Patch: a square around the blob, dragged to the texture 35
        // rows up. Its capture previews the float plate.
        let corners = [CGPoint(x: 90, y: 70), CGPoint(x: 116, y: 70), CGPoint(x: 116, y: 92),
                       CGPoint(x: 90, y: 92)]
        guard let region = CanvasSelection(shape: .polygon(corners), canvasWidth: width,
                                           canvasHeight: height) else {
            throw ProbeFailure(description: "[float retouch] the Patch region fixture")
        }
        let up = CGVector(dx: 0, dy: -35)
        controller.selectTool(.patch)
        AutomationAlerts.reset()
        try retouchDrag(controller, region, offset: up)
        guard let patch = DocumentJobs.active, patch.title == "Patch" else {
            throw ProbeFailure(description: "[float retouch] the Patch commit started no job: "
                + "\(AutomationAlerts.recorded)")
        }
        try pumpUntil({ patch.state != "running" })
        try require(patch.state == "completed" && document.doc !== base,
                    "[float retouch] the Patch completed: \(patch.report)")
        let patched = try retouchPixel(document, blobCentre.x, blobCentre.y)
        try require(retouchNear(patched, scene(blobCentre.x), 0.02),
                    "[float retouch] the Patch restores the scene: \(patched)")
        document.undoManager?.undo()
        try require(document.doc === base && document.undoManager?.canUndo != true,
                    "[float retouch] the Patch is exactly one undo")
        canvas.patchSession.cancel()

        // (f) Remove: the painted stroke is one Content-Aware Fill job.
        controller.selectTool(.remove)
        controller.removeStrokeCommitted(RemoveSession.Result(
            points: [CGPoint(x: 98.5, y: 80.5), CGPoint(x: 106.5, y: 80.5)], size: 20))
        let remove = try finishedRetouchJob("[float retouch] the Remove stroke")
        try require(remove.state == "completed" && document.doc !== base,
                    "[float retouch] the Remove stroke completed: \(remove.report)")
        let removed = try retouchPixel(document, blobCentre.x, blobCentre.y)
        try require(retouchNear(removed, scene(blobCentre.x), 0.05),
                    "[float retouch] Remove restores the scene: \(removed)")
        document.undoManager?.undo()
        try require(document.doc === base && document.undoManager?.canUndo != true,
                    "[float retouch] the Remove stroke is exactly one undo")

        // (g) Content-Aware Move of the blob's square, 35 rows up.
        controller.selectTool(.contentAwareMove)
        AutomationAlerts.reset()
        try retouchDrag(controller, region, offset: up)
        guard let move = DocumentJobs.active, move.title == "Content-Aware Move" else {
            throw ProbeFailure(description: "[float retouch] the Content-Aware Move commit started "
                + "no job: \(AutomationAlerts.recorded)")
        }
        try pumpUntil({ move.state != "running" })
        try require(move.state == "completed" && document.doc !== base,
                    "[float retouch] the Content-Aware Move completed: \(move.report)")
        let landed = try retouchPixel(document, blobCentre.x, blobCentre.y - 35)
        try require(retouchNear(landed, [0.01, 0.01, 0.01, 1], 0.05),
                    "[float retouch] the moved blob lands 35 rows up: \(landed)")
        document.undoManager?.undo()
        try require(document.doc === base && document.undoManager?.canUndo != true,
                    "[float retouch] the Content-Aware Move is exactly one undo")
        canvas.patchSession.cancel()

        // (h) Stroke Path with a foreground above white: one "Stroke Path"
        // step whose record carries `rgba32` alone.
        _ = try agentOK("set_path", document,
                        ["subpaths": [["anchors": [[30, 100], [130, 100]]]]])
        document.undoManager?.removeAllActions()
        guard let pathed = document.doc, let work = pathed.paths.entry(named: "Work Path")?.id else {
            throw ProbeFailure(description: "[float retouch] the work path was not set")
        }
        controller.setCurrentPath(.path(work))
        controller.selectTool(.brush)
        controller.paintColor = hdr
        canvas.paintColor = hdr
        let item = NSMenuItem(title: "Stroke Path",
                              action: #selector(EditorViewController.strokeCurrentPath(_:)),
                              keyEquivalent: "")
        try require(controller.validatePathsItem(item) == true,
                    "[float retouch] Stroke Path validates enabled")
        AutomationAlerts.reset()
        ActionRecorder.shared.start()
        controller.strokeCurrentPath(nil)
        let steps = ActionRecorder.shared.stop()
        try require(AutomationAlerts.recorded.isEmpty,
                    "[float retouch] Stroke Path ran with no alert: \(AutomationAlerts.recorded)")
        let strokes = steps.filter { $0.tool == "stroke_path" }
        let recorded = strokes.first?.arguments["rgba32"] as? [Double] ?? []
        try require(strokes.count == 1 && retouchNear(recorded, retouchInk)
                    && ["color", "color_space"].allSatisfy { strokes[0].arguments[$0] == nil },
                    "[float retouch] one stroke_path step carrying rgba32 alone: "
                    + "\(steps.map { "\($0.tool) \($0.arguments.keys.sorted())" })")
        let stroked = try retouchPixel(document, 80, 100)
        try require(retouchNear(stroked, retouchInk),
                    "[float retouch] the path's line reads the ink above white: \(stroked)")
        try require(document.undoManager?.undoActionName == "Stroke Path",
                    "[float retouch] the step is named Stroke Path: "
                    + "\(document.undoManager?.undoActionName ?? "")")
        document.undoManager?.undo()
        try require(document.doc === pathed && document.undoManager?.canUndo != true,
                    "[float retouch] Stroke Path is exactly one undo")

        // (i) A Clone Stamp whose stored mode is Screen starts no stroke and
        // says why.
        var screen = ToolOptionsStore.shared.clone
        screen.blendIndex = Int(DR_BLEND_SCREEN.rawValue)
        ToolOptionsStore.shared.clone = screen
        controller.selectTool(.clone)
        canvas.cloneSource = source
        AutomationAlerts.reset()
        canvas.beginStroke(at: from)
        let started = canvas.strokeActive
        if started { canvas.endStroke() }
        try require(!started && DocumentJobs.active == nil && document.doc === pathed,
                    "[float retouch] a Screen clone starts no stroke")
        try require(AutomationAlerts.recorded.count == 1
                    && AutomationAlerts.recorded.contains {
                        ($0.messageText + " " + $0.informativeText).contains("Screen blend mode")
                    },
                    "[float retouch] the refused mode is explained once: \(AutomationAlerts.recorded)")
    }
    print("PASS: float-retouch-strokes — in a 32-bit float document the Clone Stamp, Healing "
        + "Brush, Spot Healing, Patch, Remove and Content-Aware Move are admitted, as are Red Eye "
        + "and the Blur, Sharpen, Smudge and History brushes (the bucket, Dodge, the Sponge and "
        + "the Magic Wand too); a "
        + "Clone Stamp stroke shows its source overlay and source image with no alert, is one job "
        + "that lands the source's above-white values, one undo, and applies nothing when "
        + "cancelled; a Healing Brush, a Spot Healing, a Patch, a Remove and a Content-Aware Move "
        + "each complete as one undo step; Stroke Path with a (4, 0.5, 0.25) foreground is one "
        + "Stroke Path step recorded as rgba32; a Screen clone starts no stroke and says why")
}
