import AppKit

// "remove accumulation": the Remove tool with "Remove after each stroke" OFF.
// Held strokes leave the document unchanged; ✓ on a pixel-locked layer refuses and keeps both
// strokes held (✓ stays enabled); ✓ after unlocking is exactly one undo step that restores the
// base; Return applies too; ✕ and Escape leave no step and no held strokes; a tool change
// drops them. Mode: Content-Aware, so the case needs no model.

/// The Remove options for the case, restored afterwards (`withPatchOptions`' pattern).
private func withRemoveAccumulationOptions(_ body: () throws -> Void) throws {
    let store = ToolOptionsStore.shared
    let saved = (store.remove, store.removeTool)
    defer { (store.remove, store.removeTool) = saved }
    var tip = PaintToolOptions()
    tip.sample = .current
    tip.outputToNewLayer = false
    store.remove = tip
    var options = RemoveToolOptions()
    options.mode = .contentAware
    options.removeAfterEachStroke = false
    store.removeTool = options
    try body()
}

private let accumulationStrokes = [
    RemoveSession.Result(points: [CGPoint(x: 120, y: 100), CGPoint(x: 160, y: 100)], size: 16),
    RemoveSession.Result(points: [CGPoint(x: 260, y: 200), CGPoint(x: 260, y: 230)], size: 12),
]

private func holdBoth(_ controller: EditorViewController, _ tag: String) throws {
    for stroke in accumulationStrokes { controller.removeStrokeCommitted(stroke) }
    try require(controller.canvas.removeSession.pending.count == 2 && DocumentJobs.active == nil,
                "[remove accumulation] \(tag): both strokes are held and no job started")
}

private func returnKey() -> NSEvent? {
    NSEvent.keyEvent(with: .keyDown, location: .zero, modifierFlags: [],
                     timestamp: ProcessInfo.processInfo.systemUptime,
                     windowNumber: 0, context: nil, characters: "\r",
                     charactersIgnoringModifiers: "\r", isARepeat: false, keyCode: 36)
}

/// The held strokes' Apply as one job: completed, a new document, nothing held.
private func appliedJob(_ document: ImageDocument, _ controller: EditorViewController,
                        from base: RasterDocument, _ tag: String) throws {
    guard let job = DocumentJobs.active, job.title == "Remove" else {
        throw ProbeFailure(description: "[remove accumulation] \(tag) started no Remove job")
    }
    try require(!controller.canvas.removeSession.hasPending,
                "[remove accumulation] \(tag): the strokes are no longer held once the job runs")
    try pumpUntil({ job.state != "running" })
    try require(job.state == "completed" && document.doc !== base,
                "[remove accumulation] \(tag) completed: \(job.report)")
    try require(job.details["covered_pixels"] as? Int ?? 0 > 0,
                "[remove accumulation] \(tag) covered pixels: \(job.details)")
    document.undoManager?.undo()
    try require(document.doc === base && document.undoManager?.canUndo != true,
                "[remove accumulation] \(tag) is exactly one undo step restoring the base")
}

func verifyRemoveAccumulation() throws {
    let document = try fixture(width: 400, height: 300)
    defer { document.close() }
    let controller = try editor(document)
    controller.selectTool(.remove)
    try withRemoveAccumulationOptions {
        guard let base = document.doc else { throw ProbeFailure(description: "No Remove base") }

        // (a) Held strokes change nothing and record nothing.
        try holdBoth(controller, "(a)")
        try require(document.doc === base && document.undoManager?.canUndo != true,
                    "[remove accumulation] (a) held strokes leave the document unchanged")

        // (b) ✓ on a pixel-locked layer refuses and keeps both strokes held.
        _ = try agentOK("set_layer_lock", document, ["layer": 0, "locks": ["pixels"]])
        guard let locked = document.doc else { throw ProbeFailure(description: "Lock fixture") }
        AutomationAlerts.reset()
        controller.applyPendingRemove()
        try require(document.doc === locked && DocumentJobs.active == nil,
                    "[remove accumulation] (b) ✓ on a locked layer starts nothing")
        try require(AutomationAlerts.recorded.count == 1,
                    "[remove accumulation] (b) the lock refusal is one alert: "
                        + "\(AutomationAlerts.recorded)")
        try require(controller.canvas.removeSession.pending.count == 2,
                    "[remove accumulation] (b) the refused ✓ keeps both strokes held (✓ enabled)")

        // (c) ✓ after unlocking: one job, one undo step back to the unlocked base.
        _ = try agentOK("set_layer_lock", document, ["layer": 0, "locks": [String]()])
        guard let unlocked = document.doc else { throw ProbeFailure(description: "Unlock") }
        document.undoManager?.removeAllActions()
        controller.applyPendingRemove()
        try appliedJob(document, controller, from: unlocked, "(c) ✓")

        // (d) Return applies the held strokes the same way.
        try holdBoth(controller, "(d)")
        guard let enter = returnKey(), let escape = escapeKey() else {
            throw ProbeFailure(description: "Key fixtures")
        }
        controller.canvas.keyDown(with: enter)
        try appliedJob(document, controller, from: unlocked, "(d) Return")

        // (e) ✕ and Escape drop the strokes, leaving no step.
        try holdBoth(controller, "(e)")
        controller.cancelPendingRemove()
        try require(!controller.canvas.removeSession.hasPending && document.doc === unlocked
                    && document.undoManager?.canUndo != true,
                    "[remove accumulation] (e) ✕ leaves no step and no held strokes")
        try holdBoth(controller, "(e) Escape")
        controller.canvas.keyDown(with: escape)
        try require(!controller.canvas.removeSession.hasPending && document.doc === unlocked
                    && document.undoManager?.canUndo != true,
                    "[remove accumulation] (e) Escape leaves no step and no held strokes")

        // (f) A tool change drops them.
        try holdBoth(controller, "(f)")
        controller.selectTool(.select)
        try require(!controller.canvas.removeSession.hasPending && document.doc === unlocked,
                    "[remove accumulation] (f) a tool change drops the held strokes")
        controller.selectTool(.remove)
    }
    print("PASS: remove accumulation — with Remove after each stroke off two held strokes leave "
        + "the document unchanged; ✓ on a locked layer refuses and keeps both held; ✓ after "
        + "unlocking and Return are each exactly one undo step restoring the base; ✕ and Escape "
        + "leave no step and no held strokes; a tool change drops them")
}
