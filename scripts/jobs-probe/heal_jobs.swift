import AppKit
import Darwin
import ImageIO

// The 8-bit Healing Brush and Patch commit as nonmodal
// `DocumentJobs` jobs. Every case drives the canvas callbacks directly
// (`onStrokeBegin` / `onCommitStrokeOverlay` / `onStrokeEnd`, the canvas's own
// `patchSession` and `onPatchCommit`), so none needs a key window.

/// A canvas-sized premultiplied Healing Brush overlay: a centred square at
/// full coverage whose RGB is the layer's own pixels `shift` px to the right —
/// what the clone pipeline hands the commit.
func healOverlay(_ base: RasterDocument, side: Int = 120, shift: Int = 150) throws -> [UInt8] {
    guard let pixels = base.layerPixels(0) else { throw ProbeFailure(description: "Heal fixture pixels") }
    let width = base.width, height = base.height
    var overlay = [UInt8](repeating: 0, count: width * height * 4)
    let x0 = (width - side) / 2, y0 = (height - side) / 2
    for y in y0..<(y0 + side) {
        for x in x0..<(x0 + side) {
            let source = (y * width + (x + shift) % width) * 4
            let target = (y * width + x) * 4
            for c in 0..<3 { overlay[target + c] = pixels[source + c] }
            overlay[target + 3] = 255
        }
    }
    return overlay
}

/// Every layer's bytes and the layer count agree.
private func sameLayers(_ a: RasterDocument?, _ b: RasterDocument?) -> Bool {
    guard let a, let b, a.layerCount == b.layerCount else { return false }
    return (0..<a.layerCount).allSatisfy { a.layerPixels($0) == b.layerPixels($0) }
}

/// One whole healing gesture through the canvas callbacks. `between` runs
/// after the begin and before the commit.
func healGesture(_ controller: EditorViewController, _ overlay: [UInt8],
                 name: String = "Healing Brush", between: () -> Void = {}) throws {
    try require(controller.canvas.onStrokeBegin?() == true, "[\(name)] stroke begin")
    between()
    overlay.withUnsafeBufferPointer { buffer in
        if let data = buffer.baseAddress { controller.canvas.onCommitStrokeOverlay?(data, name) }
    }
    controller.canvas.onStrokeEnd?(name)
}

/// The heal store for one case, restored afterwards.
func withHealOptions(newLayer: Bool, _ body: (PaintToolOptions) throws -> Void) throws {
    let saved = ToolOptionsStore.shared.heal
    defer { ToolOptionsStore.shared.heal = saved }
    var options = saved
    options.outputToNewLayer = newLayer
    options.expandDestination = false
    options.sample = .current
    options.ignoreAdjustments = false
    ToolOptionsStore.shared.heal = options
    try body(options)
}

/// A job that holds the worker until it is cancelled, then changes nothing.
func heldJob(_ document: ImageDocument, showProgress: Bool = false) throws -> DocumentJob {
    try DocumentJobs.start(document: document, layer: 0, title: "Held Probe", scratch: 0,
                           record: .notACommand, showProgress: showProgress) { _, control in
        while !control.isCancelled { usleep(1000) }
        return nil
    }
}

private func healGestureCase(newLayer: Bool) throws {
    let label = newLayer ? "new layer" : "layer"
    let document = try fixture(width: 700, height: 600)
    defer { document.close() }
    let controller = try editor(document)
    controller.selectTool(.heal)
    try withHealOptions(newLayer: newLayer) { options in
        guard let base = document.doc else { throw ProbeFailure(description: "No heal base") }
        let overlay = try healOverlay(base)
        let strength = min(max(options.opacity, 0), 100) / 100
        let expected: RasterDocument?
        if newLayer {
            guard let plate = try base.retouchSource(0, sample: .currentAndBelow,
                                                     ignoreAdjustments: false) else {
                throw ProbeFailure(description: "[\(label)] reference plate")
            }
            expected = try base.healToNewLayerJob(0, overlay: overlay, strength: strength,
                plate: plate, name: EditorViewController.retouchLayerName,
                control: RasterJobControl())
        } else {
            expected = try base.healJob(0, overlay: overlay, strength: strength,
                                        control: RasterJobControl())
        }
        try require(expected != nil, "[\(label)] direct heal reference")

        // Completion: one worker job, the pin up and no panel at once.
        try healGesture(controller, overlay)
        guard let job = DocumentJobs.active else {
            throw ProbeFailure(description: "[\(label)] heal gesture did not start a job")
        }
        try require(!document.isLiveEditing && document.doc === base,
                    "[\(label)] heal releases the live gesture before the worker")
        try require(controller.canvas.previewImage != nil, "[\(label)] heal pins the drag's picture")
        try require(job.panel == nil, "[\(label)] no progress panel at once")
        try pumpUntil({ job.state != "running" })
        try require(job.state == "completed", "[\(label)] heal completed: \(job.report)")
        try require(sameLayers(document.doc, expected), "[\(label)] gesture heal equals the direct heal")
        try require(controller.canvas.previewImage == nil, "[\(label)] completion cleared the pin")
        document.undoManager?.undo()
        try require(document.doc === base && document.undoManager?.canUndo != true,
                    "[\(label)] exactly one undo restores the base")
        document.undoManager?.redo()
        try require(sameLayers(document.doc, expected) && document.undoManager?.canRedo != true,
                    "[\(label)] exactly one redo restores the heal")
        document.undoManager?.undo()

        // Cancel before the first poll: nothing applied, no undo, pin gone.
        try healGesture(controller, overlay)
        guard let cancelled = DocumentJobs.active else {
            throw ProbeFailure(description: "[\(label)] cancel case started no job")
        }
        cancelled.control.cancel()
        try pumpUntil({ cancelled.state != "running" })
        try require(cancelled.state == "cancelled" && document.doc === base
                    && document.undoManager?.canUndo != true,
                    "[\(label)] a cancelled heal applies nothing: \(cancelled.report)")
        try require(controller.canvas.previewImage == nil, "[\(label)] cancel cleared the pin")

        // A rename before the first poll makes the job stale.
        AutomationAlerts.reset()
        try healGesture(controller, overlay)
        guard let stale = DocumentJobs.active else {
            throw ProbeFailure(description: "[\(label)] stale case started no job")
        }
        withUndoGroup(document.undoManager) {
            document.applyEdit("Rename", record: .notACommand) { try $0.withLayerName(0, "Changed") }
        }
        let renamed = document.doc
        try pumpUntil({ stale.state != "running" })
        try require(stale.state == "stale" && document.doc === renamed,
                    "[\(label)] a heal over a changed document is stale: \(stale.report)")
        try require(controller.canvas.previewImage == nil, "[\(label)] stale cleared the pin")
        try require(AutomationAlerts.recorded.contains { $0.messageText.contains("document changed") },
                    "[\(label)] the stale heal was explained: \(AutomationAlerts.recorded)")
        document.undoManager?.undo()
    }
}

/// A channel or mask display draws the preview in place of its own base, so
/// no pin may go up while one is showing.
private func healChannelDisplayCase() throws {
    let document = try fixture(width: 700, height: 600)
    defer { document.close() }
    let controller = try editor(document)
    controller.selectTool(.heal)
    try withHealOptions(newLayer: false) { _ in
        guard let base = document.doc else { throw ProbeFailure(description: "No channel base") }
        let overlay = try healOverlay(base)
        try healGesture(controller, overlay) {
            controller.canvas.channelDisplay = ChannelDisplay(base: controller.canvas.image,
                baseKind: .plane(.red), replacesComposite: true, overlays: [])
        }
        guard let job = DocumentJobs.active else {
            throw ProbeFailure(description: "[channel] heal gesture did not start a job")
        }
        try require(controller.canvas.previewImage == nil, "[channel] no pin over a channel display")
        try pumpUntil({ job.state != "running" })
        try require(job.state == "completed", "[channel] heal completed: \(job.report)")
        controller.canvas.channelDisplay = nil
    }
}

func verifyHealGesture() throws {
    try healGestureCase(newLayer: false)
    try healGestureCase(newLayer: true)
    try healChannelDisplayCase()
    print("PASS: Healing Brush gesture runs as a nonmodal job (layer and new layer): pin, lazy "
        + "panel, direct-heal-equal pixels, one undo/redo, cancel and stale; no pin over a "
        + "channel display")
}

/// One Patch drag through the canvas's own session: press inside `region`,
/// drag `offset`, release, and hand the result to `onPatchCommit`.
private func patchDrag(_ controller: EditorViewController, _ region: CanvasSelection,
                       offset: CGFloat) throws {
    let canvas: ImageCanvasView = controller.canvas
    let from = CGPoint(x: region.bounds.midX, y: region.bounds.midY)
    let to = CGPoint(x: from.x + offset, y: from.y)
    _ = canvas.patchSession.mouseDown(from, clickCount: 1, selection: region, in: canvas.image,
        magnification: 1, sampling: { controller.patchCapture() })
    _ = canvas.patchSession.mouseDragged(to)
    guard let result = canvas.patchSession.mouseUp(to) else {
        throw ProbeFailure(description: "Patch drag produced no commit")
    }
    try require(result.capture != nil, "Patch capture was taken")
    canvas.onPatchCommit?(result)
}

private let patchPoints = [CGPoint(x: 300, y: 250), CGPoint(x: 400, y: 250),
                           CGPoint(x: 400, y: 350), CGPoint(x: 300, y: 350)]

private func patchRegion() throws -> CanvasSelection {
    guard let region = CanvasSelection(shape: .polygon(patchPoints), canvasWidth: 700,
                                       canvasHeight: 600) else {
        throw ProbeFailure(description: "Patch region fixture")
    }
    return region
}

/// The Patch store for one case, restored afterwards.
private func withPatchOptions(_ body: () throws -> Void) throws {
    let saved = ToolOptionsStore.shared.patch
    defer { ToolOptionsStore.shared.patch = saved }
    var options = saved
    options.directionIndex = 0
    options.sample = .current
    options.ignoreAdjustments = false
    ToolOptionsStore.shared.patch = options
    try body()
}

func verifyPatchCommit() throws {
    let document = try fixture(width: 700, height: 600)
    defer { document.close() }
    let controller = try editor(document)
    controller.selectTool(.patch)
    try withPatchOptions {
        guard let base = document.doc else { throw ProbeFailure(description: "No Patch base") }
        let region = try patchRegion()
        try patchDrag(controller, region, offset: 150)
        guard let job = DocumentJobs.active, job.title == "Patch" else {
            throw ProbeFailure(description: "8-bit Patch commit did not start a job")
        }
        try require(document.doc === base && job.panel == nil, "Patch job starts unpublished, no panel")
        try pumpUntil({ job.state != "running" })
        try require(job.state == "completed", "Patch completed: \(job.report)")
        guard let patched = document.doc, patched !== base else {
            throw ProbeFailure(description: "Patch changed nothing")
        }
        document.undoManager?.undo()
        try require(document.doc === base && document.undoManager?.canUndo != true,
                    "Patch is exactly one undo")

        // The MCP twin, a waiting job through the same `startByte`, on the
        // same region and offset.
        let arguments: [String: Any] = [
            "document_id": AgentServer.shared.documentID(for: document), "layer": 0,
            "points": patchPoints.map { [Double($0.x), Double($0.y)] },
            "offset_x": 150, "offset_y": 0, "direction": "source", "sample": "current",
        ]
        let encoded = try JSONSerialization.data(withJSONObject: arguments)
        let raw = AgentServer.shared.execute(tool: "patch_region",
            argumentsJSON: String(decoding: encoded, as: UTF8.self))
        try pumpUntil({ DocumentJobs.active == nil && !BackgroundWork.occupied })
        try require(sameLayers(document.doc, patched), "Patch job equals the MCP patch_region replay: \(raw)")
        document.undoManager?.undo()
        try require(document.doc === base, "MCP Patch undone")

        // Cancel before the first poll.
        try patchDrag(controller, region, offset: 150)
        guard let cancelled = DocumentJobs.active else {
            throw ProbeFailure(description: "Patch cancel case started no job")
        }
        cancelled.control.cancel()
        try pumpUntil({ cancelled.state != "running" })
        try require(cancelled.state == "cancelled" && document.doc === base
                    && document.undoManager?.canUndo != true,
                    "A cancelled Patch applies nothing: \(cancelled.report)")
        controller.canvas.patchSession.cancel()
    }
    print("PASS: 8-bit Patch commits as a nonmodal job equal to the MCP replay, one undo, cancel")
}

/// The 16-bit Destination Patch places its displaced region on the
/// job's worker instead of in a modal wait before the job.
func verifyPrecisionPatchCommit() throws {
    let document = try precisionFixture(width: 700, height: 600)
    defer { document.close() }
    let controller = try editor(document)
    controller.selectTool(.patch)
    try withPatchOptions {
        ToolOptionsStore.shared.patch.directionIndex = 1
        guard let base = document.doc else { throw ProbeFailure(description: "No 16-bit Patch base") }
        let baseSamples = try precisionLayerSamples(base)
        let region = try patchRegion()
        try patchDrag(controller, region, offset: 150)
        guard let job = DocumentJobs.active, job.title == "Patch" else {
            throw ProbeFailure(description: "16-bit Destination Patch commit did not start a job")
        }
        // The job's scratch is what its worker holds outside the core's own
        // reservation and the retained payload (the region): its placed copy.
        try require(job.state == "running" && job.scratch == UInt64(700 * 600 * 2)
                    && document.doc === base && job.panel == nil,
                    "16-bit Patch job runs unpublished with its held coverage as scratch and no panel: \(job.report)")
        try pumpUntil({ job.state != "running" })
        try require(job.state == "completed", "16-bit Patch completed: \(job.report)")
        guard let patched = document.doc, patched !== base else {
            throw ProbeFailure(description: "16-bit Patch published nothing")
        }
        let patchedSamples = try precisionLayerSamples(patched)
        try require(patchedSamples != baseSamples, "16-bit Patch changed the layer's samples")
        document.undoManager?.undo()
        try require(document.doc === base && document.undoManager?.canUndo != true,
                    "16-bit Patch is exactly one undo")

        // Cancel before the first poll.
        try patchDrag(controller, region, offset: 150)
        guard let cancelled = DocumentJobs.active else {
            throw ProbeFailure(description: "16-bit Patch cancel case started no job")
        }
        cancelled.control.cancel()
        try pumpUntil({ cancelled.state != "running" })
        try require(cancelled.state == "cancelled" && document.doc === base
                    && document.undoManager?.canUndo != true,
                    "A cancelled 16-bit Patch applies nothing: \(cancelled.report)")
        controller.canvas.patchSession.cancel()
    }
    print("PASS: 16-bit Destination Patch places its region on the job's worker: running job whose "
        + "scratch is its placed coverage, no panel, completed, one undo, cancel")
}

func verifyHealLeak() throws {
    let document = try fixture(width: 700, height: 600)
    defer { document.close() }
    let controller = try editor(document)
    guard let base = document.doc else { throw ProbeFailure(description: "No leak base") }
    let overlay = try healOverlay(base)
    guard let moved = CanvasSelection(shape: .rect(CGRect(x: 10, y: 10, width: 40, height: 40)),
                                      canvasWidth: 700, canvasHeight: 600) else {
        throw ProbeFailure(description: "Leak selection fixture")
    }
    for (tool, name) in [(EditorTool.spotHeal, "Spot Healing"), (.heal, "Healing Brush")] {
        controller.selectTool(tool)
        try withHealOptions(newLayer: false) { _ in
            try healGesture(controller, overlay, name: name) {
                withUndoGroup(document.undoManager) { document.setCanvasSelection(moved) }
            }
        }
        try require(!document.isLiveEditing && DocumentJobs.active == nil && document.doc === base,
                    "[\(name)] a stroke whose target moved closes its session and starts no job")
        withUndoGroup(document.undoManager) { document.setCanvasSelection(nil) }
    }
    print("PASS: a healing stroke whose selection changed mid-gesture leaks no live edit")
}

func verifyGestureGuard() throws {
    let a = try fixture(width: 300, height: 200)
    defer { a.close() }
    let b = try fixture(width: 300, height: 200)
    defer { b.close() }
    let editorA = try editor(a)
    let editorB = try editor(b)
    let held = try heldJob(a)
    func begins(_ controller: EditorViewController, _ tool: EditorTool) -> Bool {
        controller.selectTool(tool)
        let began = controller.canvas.onStrokeBegin?() == true
        if began { controller.canvas.onStrokeCancel?() }
        return began
    }
    try require(!begins(editorA, .heal), "Heal on the job's document is refused")
    try require(!begins(editorA, .brush), "Brush on the job's document is refused")
    try require(begins(editorB, .brush), "Brush on another document is allowed")
    try require(!begins(editorB, .heal), "Heal on another document is refused (worker busy)")

    // A Patch commit refused by the busy worker keeps its placed region.
    try withPatchOptions {
        editorA.selectTool(.patch)
        guard let region = CanvasSelection(shape: .rect(CGRect(x: 100, y: 60, width: 60, height: 60)),
                                           canvasWidth: 300, canvasHeight: 200) else {
            throw ProbeFailure(description: "Guard Patch region fixture")
        }
        let base = a.doc
        try patchDrag(editorA, region, offset: 80)
        try require(DocumentJobs.active === held && a.doc === base, "Refused Patch started nothing")
        try require(editorA.canvas.patchSession.phase == .placed, "Refused Patch keeps its region placed")
        editorA.canvas.patchSession.cancel()
    }

    held.control.cancel()
    try pumpUntil({ held.state != "running" })
    try require(begins(editorA, .heal), "Heal is allowed once the job ended")

    // Settle: a finished-but-unpolled heal is applied by the next begin.
    try withHealOptions(newLayer: false) { _ in
        editorA.selectTool(.heal)
        guard let base = a.doc else { throw ProbeFailure(description: "No settle base") }
        try healGesture(editorA, try healOverlay(base, side: 60, shift: 90))
        guard let job = DocumentJobs.active else { throw ProbeFailure(description: "Settle job missing") }
        BackgroundWork.queue.sync {}
        try require(job.state == "running", "Settle job was still unpolled")
        try require(editorA.canvas.onStrokeBegin?() == true, "Heal begin after a finished job is allowed")
        editorA.canvas.onStrokeCancel?()
        try require(job.state == "completed" && a.doc !== base,
                    "The begin applied the finished heal first: \(job.report)")
    }
    print("PASS: gesture guard refuses worker-bound and same-document strokes while a job runs, "
        + "keeps a refused Patch region, and settles a finished job first")
}

func verifyLazyPanel() throws {
    let document = try fixture(width: 64, height: 64)
    defer { document.close() }
    let job = try heldJob(document, showProgress: true)
    try require(job.panel == nil, "No panel at start")
    RunLoop.main.run(until: Date().addingTimeInterval(0.4))
    try require(job.panel != nil, "Panel appears after the delay")
    job.control.cancel()
    try pumpUntil({ job.state != "running" })
    try require(job.panel == nil && job.state == "cancelled", "Panel closed on cancel: \(job.report)")
    print("PASS: a job's progress panel appears only after \(DocumentJobs.panelDelay) s")
}

// MCP `heal_stroke` (8-bit) and `patch_region` commit through the
// same `DocumentJobs` job, waiting by default and returning a job_id with
// `background: true`.

/// One MCP call through `execute`: whether it was an in-band error, the
/// decoded JSON payload (empty for an error) and the reply's text.
func agentCall(_ tool: String, _ document: ImageDocument, _ arguments: [String: Any] = [:])
    throws -> (isError: Bool, result: [String: Any], text: String)
{
    var arguments = arguments
    arguments["document_id"] = AgentServer.shared.documentID(for: document)
    let encoded = try JSONSerialization.data(withJSONObject: arguments)
    let raw = AgentServer.shared.execute(tool: tool,
        argumentsJSON: String(decoding: encoded, as: UTF8.self))
    guard let reply = try JSONSerialization.jsonObject(with: Data(raw.utf8)) as? [String: Any],
          let content = reply["content"] as? [[String: Any]],
          let text = content.first(where: { $0["type"] as? String == "text" })?["text"] as? String else {
        throw ProbeFailure(description: "Malformed MCP response to \(tool): \(raw)")
    }
    if reply["isError"] as? Bool == true { return (true, [:], text) }
    guard let result = try JSONSerialization.jsonObject(with: Data(text.utf8)) as? [String: Any] else {
        throw ProbeFailure(description: "Missing MCP JSON text from \(tool): \(raw)")
    }
    return (false, result, text)
}

func agentOK(_ tool: String, _ document: ImageDocument,
             _ arguments: [String: Any] = [:]) throws -> [String: Any] {
    let reply = try agentCall(tool, document, arguments)
    try require(!reply.isError, "\(tool) refused: \(reply.text)")
    return reply.result
}

/// A `background: true` call's job, still running when the call returns.
private func agentBackgroundJob(_ tool: String, _ document: ImageDocument,
                                _ arguments: [String: Any]) throws -> DocumentJob {
    var arguments = arguments
    arguments["background"] = true
    let started = try agentOK(tool, document, arguments)
    guard let id = started["job_id"] as? String, let job = DocumentJobs.find(id) else {
        throw ProbeFailure(description: "[\(tool)] background call returned no job: \(started)")
    }
    try require(started["state"] as? String == "running" && DocumentJobs.active === job,
                "[\(tool)] background call returns the running job: \(started)")
    return job
}

/// The steps a recording captured while `body` ran.
private func recordedSteps(_ document: ImageDocument, _ body: () throws -> Void) throws -> [[String: Any]] {
    _ = try agentOK("start_recording", document, ["discard": true])
    try body()
    let stopped = try agentOK("stop_recording", document)
    guard let steps = stopped["steps"] as? [[String: Any]] else {
        throw ProbeFailure(description: "stop_recording returned no steps: \(stopped)")
    }
    return steps
}

private func verifyAgentHeal(_ document: ImageDocument, base: RasterDocument) throws {
    let heal: [String: Any] = ["points": [[300, 300], [400, 300]], "size": 40,
                               "source_x": 450, "source_y": 300, "sample": "current"]

    // Waiting: the job report plus ok and today's details, one undo.
    let waited = try agentOK("heal_stroke", document, heal)
    try require(waited["ok"] as? Bool == true && waited["state"] as? String == "completed"
                && waited["action"] as? String == "Healing Brush" && waited["points"] as? Int == 2
                && waited["applied"] is [String: Any],
                "Waiting heal_stroke answers the completed job: \(waited)")
    guard let healed = document.doc, healed !== base else {
        throw ProbeFailure(description: "Waiting heal_stroke changed nothing")
    }
    document.undoManager?.undo()
    try require(document.doc === base && document.undoManager?.canUndo != true,
                "Waiting heal_stroke is exactly one undo")

    // Background: returns at once with nothing applied, completes equal.
    let job = try agentBackgroundJob("heal_stroke", document, heal)
    try require(document.doc === base && !document.isLiveEditing,
                "Background heal_stroke returns before anything is applied")
    try pumpUntil({ job.state != "running" })
    try require(job.state == "completed" && sameLayers(document.doc, healed),
                "Background heal_stroke equals the waiting heal: \(job.report)")
    document.undoManager?.undo()
    try require(document.doc === base && document.undoManager?.canUndo != true,
                "Background heal_stroke is exactly one undo")

    // Background then cancel_job.
    let cancelled = try agentBackgroundJob("heal_stroke", document, heal)
    _ = try agentOK("cancel_job", document, ["job_id": cancelled.id])
    try pumpUntil({ cancelled.state != "running" })
    try require(cancelled.state == "cancelled" && document.doc === base
                && document.undoManager?.canUndo != true,
                "A cancelled background heal_stroke applies nothing: \(cancelled.report)")

    // Recording: one step with the resolved layer and no routing arguments;
    // a cancelled call records nothing.
    let steps = try recordedSteps(document) {
        let recorded = try agentBackgroundJob("heal_stroke", document, heal)
        try pumpUntil({ recorded.state != "running" })
        try require(recorded.state == "completed", "Recorded heal completed: \(recorded.report)")
    }
    let arguments = steps.first?["arguments"] as? [String: Any] ?? [:]
    try require(steps.count == 1 && steps.first?["tool"] as? String == "heal_stroke"
                && arguments["background"] == nil && arguments["document_id"] == nil
                && arguments["layer"] as? Int == 0,
                "A background heal_stroke records exactly one step: \(steps)")
    document.undoManager?.undo()
    let none = try recordedSteps(document) {
        let dropped = try agentBackgroundJob("heal_stroke", document, heal)
        _ = try agentOK("cancel_job", document, ["job_id": dropped.id])
        try pumpUntil({ dropped.state != "running" })
    }
    try require(none.isEmpty && document.doc === base, "A cancelled heal_stroke records nothing: \(none)")

    // No-op: a selection far from the stroke clips it all away.
    guard let far = CanvasSelection(shape: .rect(CGRect(x: 10, y: 10, width: 40, height: 40)),
                                    canvasWidth: 700, canvasHeight: 600) else {
        throw ProbeFailure(description: "No-op selection fixture")
    }
    document.setCanvasSelection(far)
    document.undoManager?.removeAllActions()
    let noOp = try agentOK("heal_stroke", document, heal)
    try require(noOp["ok"] as? Bool == true && noOp["changed"] as? Bool == false
                && document.doc === base && document.undoManager?.canUndo != true,
                "A waiting no-op heal_stroke answers changed:false: \(noOp)")
    let unchanged = try agentBackgroundJob("heal_stroke", document, heal)
    try pumpUntil({ unchanged.state != "running" })
    try require(unchanged.state == "unchanged" && document.doc === base,
                "A background no-op heal_stroke ends unchanged: \(unchanged.report)")
    document.setCanvasSelection(nil)
    document.undoManager?.removeAllActions()
}

/// A result `applyEdit` refuses (a persisted warp the heal would drop) is an
/// in-band error with no undo step and no recording, not a completed job.
private func verifyAgentHealRefusals(_ document: ImageDocument, base: RasterDocument) throws {
    _ = try agentOK("warp_layer", document, ["layer": 0, "operation": "set", "target": "pixels",
        "columns": 2, "rows": 2, "filter": "bilinear",
        "displacements": [[0.35, -0.27], [0.8, -0.16], [0.16, -0.06], [0.61, 0.05]]])
    guard let warped = document.doc, warped !== base else {
        throw ProbeFailure(description: "warp_layer changed nothing")
    }
    var refusal = ""
    let heal: [String: Any] = ["layer": 0, "points": [[300, 300], [400, 300]], "size": 40,
                               "source_x": 450, "source_y": 300, "sample": "current"]
    let alerts = AutomationAlerts.recorded.count
    let steps = try recordedSteps(document) {
        let reply = try agentCall("heal_stroke", document, heal)
        try require(reply.isError, "A heal over a persisted warp is refused: \(reply.text)")
        refusal = reply.text
        // The background twin ends `refused` with the sentence as its message.
        let background = try agentBackgroundJob("heal_stroke", document, heal)
        try pumpUntil({ background.state != "running" })
        try require(background.state == "refused"
                    && (background.message ?? "").contains("Bake the warp") && document.doc === warped,
                    "A background heal over a persisted warp ends refused: \(background.report)")
    }
    try require(refusal.contains("Bake the warp") && steps.isEmpty && document.doc === warped,
                "The warp refusal is in-band and records nothing: \(refusal) \(steps)")
    // An agent job's refusal is answered in band only: no alert, waiting or
    // background (a waiting call must never block on one).
    try require(AutomationAlerts.recorded.count == alerts,
                "An agent job's refusal presented no alert: \(AutomationAlerts.recorded.suffix(2))")
    // A UI-started job (not through `finishJobCall`) keeps `applyEdit`'s alert.
    let uiJob = try DocumentJobs.start(document: document, layer: 0, title: "UI Refusal Probe",
        scratch: 0, record: [], showProgress: false, rasterizesTarget: false,
        compute: { _, _ in base })
    try? DocumentJobs.wait(uiJob)
    try require(uiJob.state == "refused" && document.doc === warped
                && AutomationAlerts.recorded.count == alerts + 1
                && AutomationAlerts.recorded.last.map {
                    "\($0.messageText) \($0.informativeText)".contains("Bake the warp") } == true,
                "A UI job's refusal presents its alert once: \(uiJob.report) \(AutomationAlerts.recorded.suffix(2))")
    document.undoManager?.undo()
    try require(document.doc === base && document.undoManager?.canUndo != true,
                "The refused heal added no undo step")

}

private func verifyAgentPatch(_ document: ImageDocument, base: RasterDocument) throws {
    let patch: [String: Any] = ["points": patchPoints.map { [Double($0.x), Double($0.y)] },
                                "offset_x": 150, "offset_y": 0, "sample": "current"]
    let waited = try agentOK("patch_region", document, patch)
    try require(waited["ok"] as? Bool == true && waited["state"] as? String == "completed"
                && waited["healed_bounds"] != nil,
                "Waiting patch_region answers the completed job: \(waited)")
    guard let patched = document.doc, patched !== base else {
        throw ProbeFailure(description: "Waiting patch_region changed nothing")
    }
    document.undoManager?.undo()
    try require(document.doc === base && document.undoManager?.canUndo != true,
                "Waiting patch_region is exactly one undo")

    let job = try agentBackgroundJob("patch_region", document, patch)
    try pumpUntil({ job.state != "running" })
    try require(job.state == "completed" && sameLayers(document.doc, patched),
                "Background patch_region equals the waiting patch: \(job.report)")
    document.undoManager?.undo()
    try require(document.doc === base && document.undoManager?.canUndo != true,
                "Background patch_region is exactly one undo")

    let cancelled = try agentBackgroundJob("patch_region", document, patch)
    _ = try agentOK("cancel_job", document, ["job_id": cancelled.id])
    try pumpUntil({ cancelled.state != "running" })
    try require(cancelled.state == "cancelled" && document.doc === base,
                "A cancelled background patch_region applies nothing: \(cancelled.report)")

    var outside = patch
    outside["offset_x"] = 5000
    let refused = try agentCall("patch_region", document, outside)
    try require(refused.isError && refused.text.contains("falls outside the picture")
                && document.doc === base,
                "An off-canvas patch keeps its sentence: \(refused.text)")
}

private func verifyAgentPrecisionHeal() throws {
    let document = try precisionFixture(width: 300, height: 200)
    defer { document.close() }
    _ = try editor(document)
    guard let base = document.doc else { throw ProbeFailure(description: "No 16-bit base") }
    let heal: [String: Any] = ["points": [[100, 100], [150, 100]], "size": 20,
                               "source_x": 200, "source_y": 100]
    // A 16-bit background heal_stroke is a job like the 8-bit one.
    let healSteps = try recordedSteps(document) {
        let job = try agentBackgroundJob("heal_stroke", document, heal)
        try pumpUntil({ job.state != "running" })
        try require(job.state == "completed" && document.doc !== base,
                    "A 16-bit background heal_stroke completes: \(job.report)")
    }
    let healArguments = healSteps.first?["arguments"] as? [String: Any] ?? [:]
    try require(healSteps.count == 1 && healSteps.first?["tool"] as? String == "heal_stroke"
                && healArguments["background"] == nil && healArguments["layer"] as? Int == 0,
                "A 16-bit background heal_stroke records exactly one step: \(healSteps)")
    document.undoManager?.undo()
    try require(document.doc === base, "16-bit heal undone")
    let cancelled = try agentBackgroundJob("heal_stroke", document, heal)
    _ = try agentOK("cancel_job", document, ["job_id": cancelled.id])
    try pumpUntil({ cancelled.state != "running" })
    try require(cancelled.state == "cancelled" && document.doc === base,
                "A cancelled 16-bit background heal_stroke applies nothing: \(cancelled.report)")

    let steps = try recordedSteps(document) {
        let job = try agentBackgroundJob("patch_region", document, [
            "points": [[60, 60], [120, 60], [120, 120], [60, 120]],
            "offset_x": 100, "offset_y": 0, "sample": "current"])
        try pumpUntil({ job.state != "running" })
        try require(job.state == "completed" && document.doc !== base,
                    "A 16-bit background patch_region completes: \(job.report)")
    }
    let arguments = steps.first?["arguments"] as? [String: Any] ?? [:]
    try require(steps.count == 1 && steps.first?["tool"] as? String == "patch_region"
                && arguments["background"] == nil,
                "A 16-bit background patch_region records exactly one step: \(steps)")
    document.undoManager?.undo()
    try require(document.doc === base, "16-bit patch undone")
}

func verifyAgentHealJobs() throws {
    defer { _ = ActionRecorder.shared.stop() }
    let document = try fixture(width: 700, height: 600)
    defer { document.close() }
    _ = try editor(document)
    guard let base = document.doc else { throw ProbeFailure(description: "No agent heal base") }
    try verifyAgentHeal(document, base: base)
    try verifyAgentPatch(document, base: base)
    try verifyAgentHealRefusals(document, base: base)
    try verifyAgentPrecisionHeal()
    print("PASS: MCP heal_stroke (8-bit) and patch_region run as jobs: waiting and background "
        + "equal, one undo, cancel_job, one recorded step with the resolved layer, no-op "
        + "changed:false, warp and retained-source refusals in-band, 16-bit background "
        + "heal and patch completed and recorded once, 16-bit background heal cancelled")
}
