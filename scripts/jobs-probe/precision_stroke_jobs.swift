import AppKit
import Darwin

// Every 16-bit canvas stroke commits as ONE nonmodal `DocumentJobs` job
// — the footprint and the edit on the worker, the stroke's ghost pinned until
// the job ends. The case drives the canvas's own `beginStroke(at:)` /
// `continueStroke(to:pressure:)` / `endStroke()`, so none needs a key window.

/// One 16-bit stroke through the canvas: press, one drag, release.
private func canvasStroke(_ controller: EditorViewController, from: CGPoint, to: CGPoint) throws {
    let canvas: ImageCanvasView = controller.canvas
    canvas.beginStroke(at: from)
    try require(canvas.strokeActive, "The 16-bit \(canvas.tool) stroke began")
    canvas.continueStroke(to: to, pressure: nil)
    canvas.endStroke()
}

/// A 16-bit layer's red and green planes, read from the typed source (an
/// 8-bit `layerPixels` read answers nil at 16 bits). Main thread, no job running.
func precisionLayerSamples(_ doc: RasterDocument?, layer: Int = 0) throws -> [UInt16] {
    guard let doc else { throw ProbeFailure(description: "No 16-bit document to read") }
    return try [RasterPlane.red, .green].flatMap {
        try PrecisionScalarOperations.read(doc, target: $0.agentName, index: layer).samples
    }
}

/// Holds the serial worker queue until the gate opens, so a job started
/// meanwhile is still running whenever it is polled. No `WorkerTask`, so
/// `BackgroundWork.occupied` stays false and the stroke may start.
private func holdWorkerQueue() -> ProbeGate {
    let gate = ProbeGate()
    BackgroundWork.queue.async { while !gate.isOpen { usleep(1000) } }
    return gate
}

/// The spot-heal store for one case, restored afterwards.
private func withSpotHealOptions(_ body: () throws -> Void) throws {
    let saved = ToolOptionsStore.shared.spotHeal
    defer { ToolOptionsStore.shared.spotHeal = saved }
    var options = saved
    options.outputToNewLayer = false
    options.expandDestination = false
    options.sample = .current
    options.ignoreAdjustments = false
    ToolOptionsStore.shared.spotHeal = options
    try body()
}

func verifyPrecisionStrokeJobs() throws {
    let document = try precisionFixture(width: 300, height: 200)
    defer { document.close() }
    let controller = try editor(document)
    let canvas: ImageCanvasView = controller.canvas
    guard let base = document.doc else { throw ProbeFailure(description: "No 16-bit stroke base") }
    let baseSamples = try precisionLayerSamples(base)
    controller.selectTool(.brush)
    canvas.paintColor = NSColor(srgbRed: 0.9, green: 0.1, blue: 0.1, alpha: 1)
    canvas.brushSize = 20
    canvas.brushOpacity = 1
    let from = CGPoint(x: 60, y: 100), to = CGPoint(x: 240, y: 100)

    // (a) Running after the release: unpublished, pinned, no panel yet, and
    // a second stroke refused while the job holds the worker.
    let gate = holdWorkerQueue()
    defer { gate.open() }
    try canvasStroke(controller, from: from, to: to)
    guard let job = DocumentJobs.active, job.title == "Brush Stroke" else {
        throw ProbeFailure(description: "The 16-bit brush stroke started no Brush Stroke job: \(DocumentJobs.active?.report ?? [:])")
    }
    try require(job.state == "running" && document.doc === base && !document.isLiveEditing,
                "The stroke's job runs with nothing published: \(job.report)")
    try require(canvas.strokeGhostPin != nil, "The finished stroke's ghost is pinned")
    try require(job.panel == nil && job.showProgress, "A UI stroke job shows no panel at once but may")
    // Its scratch is what the worker holds outside the core's reservation:
    // the footprint and the joined samples (no selection: one plane each).
    try require(job.scratch == UInt64(300 * 200 * 2 * 2),
                "The stroke job's scratch is its worker-held footprint and samples: \(job.scratch)")
    let second = canvas.onStrokeBegin?() == true
    if second { canvas.onStrokeCancel?() }
    try require(!second, "A second 16-bit stroke is refused while the job runs")

    // (b) Completed: changed pixels, the pin gone, exactly one undo and redo.
    gate.open()
    try pumpUntil({ job.state != "running" })
    try require(job.state == "completed", "The brush stroke job completed: \(job.report)")
    guard let painted = document.doc, painted !== base else {
        throw ProbeFailure(description: "The 16-bit brush stroke published nothing")
    }
    let paintedSamples = try precisionLayerSamples(painted)
    try require(paintedSamples != baseSamples, "The 16-bit brush stroke changed the layer's samples")
    try require(canvas.strokeGhostPin == nil, "Completion cleared the pin")
    document.undoManager?.undo()
    try require(document.doc === base && document.undoManager?.canUndo != true,
                "The stroke is exactly one undo")
    document.undoManager?.redo()
    let redone = try precisionLayerSamples(document.doc)
    try require(redone == paintedSamples && document.undoManager?.canRedo != true,
                "The stroke is exactly one redo")
    document.undoManager?.undo()

    // (c) Cancel before the first poll: nothing applied, no undo, pin gone.
    try canvasStroke(controller, from: from, to: to)
    guard let cancelled = DocumentJobs.active else {
        throw ProbeFailure(description: "The cancel case started no job")
    }
    cancelled.control.cancel()
    try pumpUntil({ cancelled.state != "running" })
    try require(cancelled.state == "cancelled" && document.doc === base
                && document.undoManager?.canUndo != true,
                "A cancelled stroke applies nothing: \(cancelled.report)")
    try require(canvas.strokeGhostPin == nil, "Cancel cleared the pin")

    // (d) A selection change before the first poll makes the job stale,
    // explained once. Not a rename: at 16 bits `applyEdit` prepares its
    // display on the worker, so a rename is refused while the job holds it.
    guard let moved = CanvasSelection(shape: .rect(CGRect(x: 10, y: 10, width: 40, height: 40)),
                                      canvasWidth: 300, canvasHeight: 200) else {
        throw ProbeFailure(description: "Stale selection fixture")
    }
    AutomationAlerts.reset()
    try canvasStroke(controller, from: from, to: to)
    guard let stale = DocumentJobs.active else {
        throw ProbeFailure(description: "The stale case started no job")
    }
    withUndoGroup(document.undoManager) { document.setCanvasSelection(moved) }
    try pumpUntil({ stale.state != "running" })
    try require(stale.state == "stale" && document.doc === base,
                "A stroke over a changed selection is stale: \(stale.report)")
    try require(canvas.strokeGhostPin == nil, "Stale cleared the pin")
    try require(AutomationAlerts.recorded.contains { $0.messageText.contains("document changed") },
                "The stale stroke was explained: \(AutomationAlerts.recorded)")
    withUndoGroup(document.undoManager) { document.setCanvasSelection(nil) }
    document.undoManager?.removeAllActions()

    // (e) Spot Healing takes the same path.
    try withSpotHealOptions {
        controller.selectTool(.spotHeal)
        canvas.brushSize = 20
        try canvasStroke(controller, from: CGPoint(x: 130, y: 100), to: CGPoint(x: 170, y: 100))
        guard let spot = DocumentJobs.active, spot.title == "Spot Healing" else {
            throw ProbeFailure(description: "The 16-bit spot stroke started no Spot Healing job: \(DocumentJobs.active?.report ?? [:])")
        }
        try pumpUntil({ spot.state != "running" })
        try require(spot.state == "completed" && document.doc !== base,
                    "The 16-bit spot stroke completed: \(spot.report)")
        try require(canvas.strokeGhostPin == nil, "The spot job cleared its pin")
        document.undoManager?.undo()
        try require(document.doc === base, "The spot stroke is one undo")
    }
    print("PASS: 16-bit Brush and Spot Healing strokes commit as nonmodal jobs: pinned ghost, lazy "
        + "panel, second stroke refused, one undo/redo, cancel and stale endings clear the pin")
}
