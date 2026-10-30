import AppKit

// History navigation and MCP `restore_history` during a job follow ⌘Z's and
// MCP `undo`'s rules, Esc that nothing else in the editor window consumed
// cancels the running UI job, a refused publication leaves no empty undo step,
// and MCP `undo`/`redo` answer from whether history moved.
// Every case calls methods directly (the History table's action, the
// window's `keyDown`, the MCP dispatcher), so none needs a key window.

private func historyTable(_ controller: HistoryWindowController) throws -> NSTableView {
    func find(_ view: NSView) -> NSTableView? {
        (view as? NSTableView) ?? view.subviews.lazy.compactMap(find).first
    }
    guard let content = controller.window?.contentView, let table = find(content) else {
        throw ProbeFailure(description: "History panel table")
    }
    return table
}

private func choose(_ table: NSTableView, row: Int) {
    table.selectRowIndexes(IndexSet(integer: row), byExtendingSelection: false)
    _ = table.sendAction(table.action, to: table.target)
}

func verifyHistoryDuringJob() throws {
    let document = try fixture(width: 700, height: 600)
    defer { document.close() }
    _ = try editor(document)
    guard let base = document.doc else { throw ProbeFailure(description: "History fixture") }
    let renamed = try rename(document)
    let panel = HistoryWindowController(document: document)
    let table = try historyTable(panel)
    let ids = document.history.entries.map(\.id)
    try require(ids.count == 2 && document.history.cursor == 1,
                "Two history states, cursor on the rename: \(ids.count) / \(document.history.cursor)")

    // (a) A UI job: choosing the current row does nothing; choosing another
    // row cancels the job instead of navigating.
    let a = try heldJob(document, showProgress: true)
    choose(table, row: 1)
    try require(a.state == "running" && !a.control.isCancelled && document.doc === renamed,
                "(a) choosing the current state left the UI job running")
    choose(table, row: 0)
    try require(a.control.isCancelled, "(a) choosing another state cancelled the UI job")
    try require(document.doc === renamed && document.history.cursor == 1 && table.selectedRow == 1,
                "(a) nothing navigated and the table shows the current state again: "
                    + "cursor \(document.history.cursor), row \(table.selectedRow)")
    try drain(a)
    choose(table, row: 0)
    try require(document.doc === base, "(a) after the job, row 0 navigates to the base")
    choose(table, row: 1)
    try require(document.doc === renamed, "(a) and row 1 back to the rename")

    // (b) An agent's job is not the user's: the panel navigates over it.
    let b = try heldJob(document, showProgress: false)
    choose(table, row: 0)
    try require(!b.control.isCancelled && document.doc === base,
                "(b) over an MCP job the panel navigates and leaves the job running")
    try drain(b)
    choose(table, row: 1)
    try require(document.doc === renamed, "(b) row 1 restores the rename")

    // (c)/(d) MCP restore_history refuses by name while any job runs.
    func refusesRestore(_ job: DocumentJob, _ label: String) throws {
        let reply = try agentCall("restore_history", document, ["state_id": ids[0]])
        try require(reply.isError && reply.text.contains("still running")
                    && reply.text.contains(job.id) && reply.text.contains("restore_history"),
                    "[\(label)] restore_history refuses by name: \(reply.text)")
        try require(document.history.cursor == 1 && document.doc === renamed,
                    "[\(label)] the refusal moved no history")
        try require(job.state == "running" && !job.control.isCancelled,
                    "[\(label)] the refusal left the job running")
        _ = try agentOK("cancel_job", document, ["job_id": job.id])
        try pumpUntil({ job.state != "running" })
        _ = try agentOK("restore_history", document, ["state_id": ids[0]])
        try require(document.doc === base, "[\(label)] after the job, restore_history navigates")
    }
    try refusesRestore(try heldJob(document, showProgress: true), "(c) UI job")
    _ = try agentOK("restore_history", document, ["state_id": ids[1]])
    try require(document.doc === renamed, "(d) back on the rename")
    let d = try heldJob(document, showProgress: false)
    d.agentOrigin = true
    try refusesRestore(d, "(d) MCP job")
    print("PASS: choosing a History state cancels a running UI job (the current row does "
        + "nothing), navigates over an MCP job, and MCP restore_history refuses by job_id "
        + "while any job runs")
}

func verifyWindowEscape() throws {
    let document = try fixture(width: 700, height: 600)
    defer { document.close() }
    _ = try editor(document)
    _ = try select(document, side: 120)
    guard let window = document.windowControllers.first?.window as? EditorWindow,
          let escape = escapeKey() else {
        throw ProbeFailure(description: "Editor window is an EditorWindow")
    }

    // (a) The window itself first responder (after an options-bar edit).
    let a = try heldJob(document, showProgress: true)
    window.makeFirstResponder(nil)
    window.keyDown(with: escape)
    try require(a.control.isCancelled, "(a) Esc outside the canvas cancelled the UI job")
    try require(document.canvasSelection != nil, "(a) Esc kept the selection")
    window.keyDown(with: escape)
    try require(document.canvasSelection != nil && document.undoManager?.canUndo != true,
                "(a) a second Esc while the job drains changes nothing")
    try pumpUntil({ a.state != "running" })

    // (b) No job: Esc from a table outside the canvas goes on as before;
    // nothing deselects. (The window itself as first responder now hands the
    // key to the canvas, so a throwaway table stands in, as (c) adds
    // its text view.)
    let table = NSTableView(frame: NSRect(x: 0, y: 0, width: 80, height: 40))
    table.addTableColumn(NSTableColumn(identifier: NSUserInterfaceItemIdentifier("probe")))
    window.contentView?.addSubview(table)
    defer { table.removeFromSuperview() }
    try require(window.makeFirstResponder(table) && window.firstResponder === table,
                "(b) table first responder")
    window.keyDown(with: escape)
    table.keyDown(with: escape)
    try require(document.canvasSelection != nil, "(b) Esc with no job keeps the selection")
    window.makeFirstResponder(nil)

    // (c) A text view keeps its Esc.
    let c = try heldJob(document, showProgress: true)
    let text = NSTextView(frame: NSRect(x: 0, y: 0, width: 80, height: 20))
    window.contentView?.addSubview(text)
    defer { text.removeFromSuperview() }
    try require(window.makeFirstResponder(text), "(c) text view first responder")
    text.keyDown(with: escape)
    window.keyDown(with: escape)
    try require(c.state == "running" && !c.control.isCancelled,
                "(c) Esc in a text view left the UI job running")
    window.makeFirstResponder(nil)
    try drain(c)

    // (d) An agent's job is not the user's.
    let d = try heldJob(document, showProgress: false)
    window.keyDown(with: escape)
    try require(!d.control.isCancelled, "(d) window Esc left the MCP job running")
    try drain(d)
    print("PASS: Esc outside the canvas cancels the running UI job (a second Esc is "
        + "swallowed), leaves text views and MCP jobs alone, and does nothing with no job")
}

func verifyNoEmptyUndoStep() throws {
    let document = try fixture(width: 64, height: 64)
    defer { document.close() }
    _ = try editor(document)
    guard let base = document.doc else { throw ProbeFailure(description: "Empty step fixture") }
    AutomationAlerts.reset()

    func unchanged(_ label: String) throws {
        try require(document.doc === base && document.undoManager?.canUndo != true
                    && !document.isDocumentEdited,
                    "[\(label)] no undo step, no change and no dirty state")
        try require(AutomationAlerts.recorded.isEmpty, "[\(label)] no alert: \(AutomationAlerts.recorded)")
    }

    // (a) performGroupedEdit answers the depth refusal before its group.
    do {
        try AgentServer.shared.performGroupedEdit(document, "Depth Probe") {
            try RasterDocument(precision: $0.promotedStructure())
        }
        throw ProbeFailure(description: "(a) the depth-changing edit was not refused")
    } catch let error as AgentServer.ToolError {
        try require(error.message == "The edit was refused and nothing changed.",
                    "(a) performGroupedEdit refuses by the depth sentence: \(error.message)")
    }
    try unchanged("(a) performGroupedEdit")

    // (b) applyPrepared runs the same preflight before its group.
    do {
        let promoted = try RasterDocument(precision: base.promotedStructure())
        try document.applyPrepared(PreparedDocumentEdit(promoted), captured: base,
                                   revision: document.revision, actionName: "Depth Probe",
                                   record: .notACommand, changesCanvasGeometry: false,
                                   resultingSelection: nil)
        throw ProbeFailure(description: "(b) the depth-changing prepared edit was not refused")
    } catch let error as SilentEditRefusal {
        try require(error.message == "The edit was refused and nothing changed.",
                    "(b) applyPrepared refuses by the depth sentence: \(error.message)")
    }
    try unchanged("(b) applyPrepared")
    print("PASS: a depth refusal in performGroupedEdit and applyPrepared leaves no empty undo "
        + "step, no dirty state and no alert")
}

func verifyTruthfulAgentUndo() throws {
    let document = try fixture(width: 64, height: 64)
    defer { document.close() }
    _ = try editor(document)
    guard let base = document.doc else { throw ProbeFailure(description: "Agent undo fixture") }

    withUndoGroup(document.undoManager) {}
    try require(document.undoManager?.canUndo == true,
                "The premise: an empty explicit group is an Undo step")
    let first = try agentCall("undo", document)
    try require(first.isError && first.text.contains("Nothing was undone") && document.doc === base,
                "MCP undo of an empty step answers an error: \(first.text)")

    let renamed = try rename(document)
    withUndoGroup(document.undoManager) {}
    let second = try agentCall("undo", document)
    try require(second.isError && second.text.contains("Nothing was undone") && document.doc === renamed,
                "MCP undo of an empty step over a rename answers an error: \(second.text)")
    _ = try agentOK("undo", document)
    try require(document.doc === base, "the next MCP undo pops the rename")
    _ = try agentOK("redo", document)
    try require(document.doc === renamed, "MCP redo restores the rename")
    print("PASS: MCP undo answers an error, not ok, for a step that held no document change")
}
