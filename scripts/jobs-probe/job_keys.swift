import AppKit

// Esc and ⌘Z/⇧⌘Z cancel the document's running UI-started job, MCP
// `undo`/`redo` refuse by name while any job runs, and a job result
// `applyEdit` would beep-refuse leaves no empty undo step. Every case calls
// `canvas.keyDown(with:)`, `undo(nil)`/`redo(nil)` and
// `validateUserInterfaceItem` directly, so none needs a key window.

func escapeKey() -> NSEvent? {
    NSEvent.keyEvent(with: .keyDown, location: .zero, modifierFlags: [],
                     timestamp: ProcessInfo.processInfo.systemUptime,
                     windowNumber: 0, context: nil, characters: "\u{1b}",
                     charactersIgnoringModifiers: "\u{1b}", isARepeat: false,
                     keyCode: 53)
}

func drain(_ job: DocumentJob) throws {
    job.control.cancel()
    try pumpUntil({ job.state != "running" })
}

func rename(_ document: ImageDocument) throws -> RasterDocument {
    withUndoGroup(document.undoManager) {
        document.applyEdit("Rename", record: .notACommand) { try $0.withLayerName(0, "Changed") }
    }
    guard let renamed = document.doc else { throw ProbeFailure(description: "Rename fixture") }
    return renamed
}

func verifyJobEscape() throws {
    let document = try fixture(width: 700, height: 600)
    defer { document.close() }
    let controller = try editor(document)
    controller.selectTool(.select)
    _ = try select(document, side: 120)
    guard let base = document.doc, let escape = escapeKey() else {
        throw ProbeFailure(description: "Escape fixture")
    }
    let canvas: ImageCanvasView = controller.canvas

    // (a) Esc cancels a UI job and keeps the selection; a second Esc while it
    // drains is swallowed too.
    let a = try heldJob(document, showProgress: true)
    canvas.keyDown(with: escape)
    try require(a.control.isCancelled, "(a) Esc cancelled the UI job")
    try require(document.canvasSelection != nil, "(a) Esc kept the selection")
    canvas.keyDown(with: escape)
    try require(document.canvasSelection != nil && document.undoManager?.canUndo != true,
                "(a) a second Esc while the job drains neither deselects nor records a step")
    try pumpUntil({ a.state != "running" })
    try require(a.state == "cancelled" && document.doc === base && document.undoManager?.canUndo != true,
                "(a) the job ends cancelled with nothing applied: \(a.report)")

    // (b) Esc with no job still deselects.
    canvas.keyDown(with: escape)
    try require(document.canvasSelection == nil, "(b) Esc with no job deselects")

    // (c) A crop session owns Esc ahead of the job; back on the marquee tool,
    // Esc cancels it.
    _ = try select(document, side: 120)
    let c = try heldJob(document, showProgress: true)
    controller.selectTool(.crop)
    try require(canvas.cropOverlay != nil, "(c) crop session open")
    canvas.keyDown(with: escape)
    try require(c.state == "running" && !c.control.isCancelled, "(c) Esc reset the crop box only")
    controller.selectTool(.select)
    canvas.keyDown(with: escape)
    try require(c.control.isCancelled, "(c) Esc cancels the job once the crop session is gone")
    try pumpUntil({ c.state != "running" })

    // (d) An agent-started job is not the user's: Esc deselects.
    _ = try select(document, side: 120)
    let d = try heldJob(document, showProgress: false)
    canvas.keyDown(with: escape)
    try require(!d.control.isCancelled, "(d) Esc left the MCP job running")
    try require(document.canvasSelection == nil, "(d) Esc deselected over an MCP job")
    try drain(d)
    print("PASS: Esc cancels the running UI job (a second Esc is swallowed), still deselects with "
        + "no job or over an MCP job, and a crop session keeps Esc first")
}

func verifyUndoKeysCancelJob() throws {
    let document = try fixture(width: 700, height: 600)
    defer { document.close() }
    let controller = try editor(document)
    guard let base = document.doc else { throw ProbeFailure(description: "Undo keys fixture") }
    let undoItem = NSMenuItem(title: "Undo", action: #selector(EditorViewController.undo(_:)),
                              keyEquivalent: "z")

    // (a) The first job on a fresh document: Undo reads "Cancel …" and cancels.
    let a = try heldJob(document, showProgress: true)
    try require(controller.validateUserInterfaceItem(undoItem) && undoItem.title == "Cancel Held Probe",
                "(a) Undo validates as Cancel Held Probe: \(undoItem.title)")
    controller.undo(nil)
    try require(a.control.isCancelled, "(a) ⌘Z cancelled the job")
    try pumpUntil({ a.state != "running" })
    try require(a.state == "cancelled" && document.undoManager?.canUndo != true,
                "(a) no undo step after the cancelled job: \(a.report)")
    try require(!controller.validateUserInterfaceItem(undoItem), "(a) Undo disabled again")

    // (b) A real Healing Brush job: ⌘Z cancels it, the rename stays, the pin
    // clears; a second ⌘Z pops the rename.
    let renamed = try rename(document)
    controller.selectTool(.heal)
    try withHealOptions(newLayer: false) { _ in
        try healGesture(controller, try healOverlay(renamed))
        guard let heal = DocumentJobs.active else {
            throw ProbeFailure(description: "(b) heal gesture started no job")
        }
        controller.undo(nil)
        try require(heal.control.isCancelled && document.doc === renamed,
                    "(b) ⌘Z cancelled the heal and popped nothing")
        try pumpUntil({ heal.state != "running" })
        try require(heal.state == "cancelled" && controller.canvas.previewImage == nil,
                    "(b) the heal ends cancelled with its pin cleared: \(heal.report)")
    }
    controller.undo(nil)
    try require(document.doc === base, "(b) the next ⌘Z pops the rename")

    // (c) ⇧⌘Z cancels too and leaves Redo standing.
    let c = try heldJob(document, showProgress: true)
    controller.redo(nil)
    try require(c.control.isCancelled && document.doc === base
                && document.undoManager?.canRedo == true,
                "(c) ⇧⌘Z cancelled the job and redid nothing")
    try pumpUntil({ c.state != "running" })
    controller.redo(nil)
    try require(document.doc === renamed, "(c) the next ⇧⌘Z redoes the rename")

    // (d) Over an agent-started job ⌘Z pops a step as before.
    let d = try heldJob(document, showProgress: false)
    controller.undo(nil)
    try require(!d.control.isCancelled && document.doc === base,
                "(d) ⌘Z over an MCP job pops the rename and leaves the job")
    try drain(d)
    print("PASS: ⌘Z/⇧⌘Z cancel the running UI job (held and real Healing Brush) instead of "
        + "moving history, Undo reads Cancel <title>, and an MCP job leaves ⌘Z popping steps")
}

func verifyAgentUndoDuringJob() throws {
    let document = try fixture(width: 700, height: 600)
    defer { document.close() }
    _ = try editor(document)
    guard let base = document.doc else { throw ProbeFailure(description: "Agent undo fixture") }
    let renamed = try rename(document)

    func refuses(_ job: DocumentJob, _ label: String) throws {
        for tool in ["undo", "redo"] {
            let reply = try agentCall(tool, document)
            try require(reply.isError && reply.text.contains("still running")
                        && reply.text.contains(job.id),
                        "[\(label)] MCP \(tool) refuses by name: \(reply.text)")
        }
        try require(job.state == "running" && !job.control.isCancelled,
                    "[\(label)] the refusal left the job running")
    }

    let ui = try heldJob(document, showProgress: true)
    try refuses(ui, "UI job")
    try require(document.doc === renamed, "[UI job] nothing undone")
    _ = try agentOK("cancel_job", document, ["job_id": ui.id])
    try pumpUntil({ ui.state != "running" })
    _ = try agentOK("undo", document)
    try require(document.doc === base, "MCP undo after the job pops the rename")

    let agent = try heldJob(document, showProgress: false)
    agent.agentOrigin = true
    try refuses(agent, "MCP job")
    try require(document.doc === base, "[MCP job] nothing redone")
    try drain(agent)
    _ = try agentOK("redo", document)
    try require(document.doc === renamed, "MCP redo after the job restores the rename")
    print("PASS: MCP undo/redo refuse by name while a UI or MCP job runs, and step normally after")
}

func verifyRefusedJobLeavesNoStep() throws {
    let document = try fixture(width: 64, height: 64)
    defer { document.close() }
    _ = try editor(document)
    guard let base = document.doc else { throw ProbeFailure(description: "Refused job fixture") }
    AutomationAlerts.reset()
    let job = try DocumentJobs.start(document: document, layer: 0, title: "Depth Probe", scratch: 0,
                                     record: .notACommand, showProgress: false) { captured, control in
        try RasterDocument(precision: captured.promotedStructure(control: control))
    }
    try pumpUntil({ job.state != "running" })
    try require(job.state == "refused" && job.message == "The edit was refused and nothing changed.",
                "A depth-changing job result is refused: \(job.report)")
    try require(document.doc === base && document.undoManager?.canUndo != true
                && !document.isDocumentEdited,
                "The refused job left no undo step and no dirty state")
    try require(AutomationAlerts.recorded.isEmpty, "No alert: \(AutomationAlerts.recorded)")
    print("PASS: a job result applyEdit's depth guard refuses ends refused with no empty undo step")
}
