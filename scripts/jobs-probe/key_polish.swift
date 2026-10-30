import AppKit

// An agent's refused or no-op canvas-session commit leaves no
// undo step, keys reach the canvas when the editor window itself holds
// focus, Esc from History, Snapshots, Layer Comps and Actions cancels the
// running UI job, a presented `SilentEditRefusal` beeps instead of alerting,
// MCP `set_layer_properties open` adds no undo step, and an agent guide edit
// refused mid-gesture answers an error. Every case calls methods directly
// (`keyDown(with:)`, the MCP dispatcher, the document), so none needs a key
// window.
//
// The Layers and Channels tables hand a bare key they have no use for
// to the canvas ("panel table keys"), a key that reaches the window or a
// panel table during a text session goes to the session's text view ("text
// session window keys"), ⌘Z/⇧⌘Z in History, Snapshots and Layer Comps are the
// editor's Undo/Redo ("floating panel undo"), and Esc is the key equivalent
// of a cancellable progress panel's Cancel ("progress panel escape").

private func key(_ characters: String, code: UInt16,
                 modifiers: NSEvent.ModifierFlags = []) -> NSEvent? {
    NSEvent.keyEvent(with: .keyDown, location: .zero, modifierFlags: modifiers,
                     timestamp: ProcessInfo.processInfo.systemUptime,
                     windowNumber: 0, context: nil, characters: characters,
                     charactersIgnoringModifiers: characters, isARepeat: false,
                     keyCode: code)
}

private func firstTable(_ view: NSView) -> NSTableView? {
    (view as? NSTableView) ?? view.subviews.lazy.compactMap(firstTable).first
}

func verifySessionCommitNoEmptyStep() throws {
    let document = try fixture(width: 64, height: 64)
    defer { document.close() }
    let controller = try editor(document)
    guard let base = document.doc else { throw ProbeFailure(description: "Session commit fixture") }

    // (a) An empty body on a fresh document leaves no step.
    document.withUndoGroupDiscardingEmpty {}
    try require(document.undoManager?.canUndo != true, "(a) an empty body left no undo step")

    // (b) After a rename and its undo, an empty body keeps Redo.
    let renamed = try rename(document)
    document.undoManager?.undo()
    try require(document.doc === base, "(b) undo popped the rename")
    document.withUndoGroupDiscardingEmpty {}
    try require(document.undoManager?.canRedo == true && document.undoManager?.canUndo != true,
                "(b) the empty body kept Redo and added no Undo")
    document.undoManager?.redo()
    try require(document.doc === renamed, "(b) redo restores the rename")
    document.undoManager?.undo()
    document.undoManager?.removeAllActions()
    document.history.reset(document.captureEditingState())

    // (c) An untouched Free Transform committed by an MCP writer leaves no step.
    controller.freeTransform(nil)
    try require(controller.transformSession != nil && document.hasPendingCanvasSession,
                "(c) Free Transform session open")
    _ = try agentOK("save_copy", document,
                    ["path": directory.appendingPathComponent("pending.rz").path])
    try require(controller.transformSession == nil && !document.hasPendingCanvasSession,
                "(c) save_copy closed the session")
    try require(document.undoManager?.canUndo != true && document.doc === base,
                "(c) the untouched commit left no undo step")
    // …and over a prior rename, the next undo is the rename.
    let renamedAgain = try rename(document)
    controller.freeTransform(nil)
    try require(controller.transformSession != nil, "(c) second Free Transform session open")
    _ = try agentOK("save_copy", document,
                    ["path": directory.appendingPathComponent("pending2.rz").path])
    try require(controller.transformSession == nil && document.doc === renamedAgain,
                "(c) the second commit closed the session and changed nothing")
    document.undoManager?.undo()
    try require(document.doc === base && document.undoManager?.canUndo != true,
                "(c) one undo returns to the base and nothing is left to undo")
    document.undoManager?.redo()

    // (d) Nested inside an outer group, the helper rebuilds nothing.
    document.undoManager?.undo()
    document.undoManager?.removeAllActions()
    document.history.reset(document.captureEditingState())
    withUndoGroup(document.undoManager) {
        document.applyEdit("Rename", record: .notACommand) { try $0.withLayerName(0, "Nested") }
        document.withUndoGroupDiscardingEmpty {}
    }
    try require(document.doc?.layerInfo(0)?.name == "Nested", "(d) the outer rename landed")
    document.undoManager?.undo()
    try require(document.doc === base && document.undoManager?.canUndo != true,
                "(d) the outer rename is one undo step")
    print("PASS: session commit no empty step — an empty discarding group adds no step and keeps "
        + "Redo, an untouched session committed by save_copy leaves none, and a nested one is the "
        + "outer group's")
}

func verifyWindowKeysToCanvas() throws {
    let document = try fixture(width: 200, height: 160)
    defer { document.close() }
    let controller = try editor(document)
    guard let window = document.windowControllers.first?.window as? EditorWindow,
          let v = key("v", code: 9), let escape = escapeKey() else {
        throw ProbeFailure(description: "Editor window is an EditorWindow")
    }
    let canvas: ImageCanvasView = controller.canvas

    // (a) The window itself first responder: a bare tool key reaches the canvas.
    controller.selectTool(.select)
    window.makeFirstResponder(nil)
    try require(window.firstResponder === window, "(a) the window is first responder")
    window.keyDown(with: v)
    try require(controller.currentTool == .move, "(a) V selected Move: \(controller.currentTool)")
    try require(window.firstResponder === canvas, "(a) the canvas is first responder again")

    // (b) Esc with a selection deselects, as on the canvas.
    controller.selectTool(.select)
    _ = try select(document, side: 40)
    window.makeFirstResponder(nil)
    window.keyDown(with: escape)
    try require(document.canvasSelection == nil, "(b) Esc from the window deselected")

    // (c) An open text session keeps keys off the canvas.
    controller.selectTool(.select)
    canvas.beginTextSession(at: CGPoint(x: 20, y: 20))
    defer { canvas.cancelTextSession() }
    try require(canvas.hasActiveTextSession, "(c) text session open")
    window.makeFirstResponder(nil)
    window.keyDown(with: v)
    try require(controller.currentTool == .select && canvas.hasActiveTextSession,
                "(c) V changed nothing: \(controller.currentTool)")
    print("PASS: window keys to canvas — with the editor window itself first responder, a tool "
        + "key and Esc reach the canvas, and an open text session keeps them off it")
}

func verifyFloatingWindowEscape() throws {
    let document = try fixture(width: 200, height: 160)
    defer { document.close() }
    let controller = try editor(document)
    guard let escape = escapeKey(), let editorWindow = document.windowControllers.first?.window else {
        throw ProbeFailure(description: "Floating escape fixture")
    }
    let history = HistoryWindowController(document: document)
    let snapshots = SnapshotsWindowController(document: document)
    let comps = LayerCompsWindowController(document: document)
    let actions = ActionsWindowController.shared
    try require(ActionPlayer.frontDocument() === document, "Play's target is the fixture")

    enum Route { case table(NSTableView), controller(NSResponder) }
    var cases: [(String, NSWindow, Route)] = []
    for (name, windowController) in [("History", history as NSWindowController),
                                     ("Snapshots", snapshots), ("Layer Comps", comps)] {
        guard let window = windowController.window, let content = window.contentView,
              let table = firstTable(content) else {
            throw ProbeFailure(description: "[\(name)] panel and its table")
        }
        try require(window is JobEscapePanel, "[\(name)] is a JobEscapePanel")
        cases.append((name, window, .table(table)))
    }
    guard let actionsWindow = actions.window, let actionsContent = actions.contentViewController else {
        throw ProbeFailure(description: "[Actions] window")
    }
    try require(actionsWindow is JobEscapeWindow, "[Actions] is a JobEscapeWindow")
    cases.append(("Actions", actionsWindow, .controller(actionsContent)))
    // A table with no column refuses first responder (the window keeps it).
    let editorTable = NSTableView(frame: NSRect(x: 0, y: 0, width: 40, height: 40))
    editorTable.addTableColumn(NSTableColumn(identifier: NSUserInterfaceItemIdentifier("probe")))
    editorWindow.contentView?.addSubview(editorTable)
    defer { editorTable.removeFromSuperview() }
    cases.append(("Editor", editorWindow, .table(editorTable)))

    for (name, window, route) in cases {
        controller.selectTool(.select)
        _ = try select(document, side: 40)

        // (a) The window itself first responder.
        let a = try heldJob(document, showProgress: true)
        window.makeFirstResponder(nil)
        window.keyDown(with: escape)
        try require(a.control.isCancelled, "[\(name)] (a) Esc cancelled the UI job")
        // A second Esc is swallowed while the job winds down: it must not
        // fall through to Deselect.
        window.makeFirstResponder(nil)
        window.keyDown(with: escape)
        try require(document.canvasSelection != nil && document.undoManager?.canUndo != true,
                    "[\(name)] (a) Esc kept the selection and recorded nothing")
        try pumpUntil({ a.state != "running" })

        // (b) The real responder dispatch.
        let b = try heldJob(document, showProgress: true)
        switch route {
        case .table(let table):
            try require(window.makeFirstResponder(table) && window.firstResponder === table,
                        "[\(name)] (b) table first responder")
            table.keyDown(with: escape)
        case .controller(let responder):
            try require(window.makeFirstResponder(responder) && window.firstResponder === responder,
                        "[\(name)] (b) controller first responder")
            responder.keyDown(with: escape)
        }
        try require(b.control.isCancelled, "[\(name)] (b) Esc through the responder chain cancelled the job")
        try require(document.canvasSelection != nil, "[\(name)] (b) Esc kept the selection")
        try pumpUntil({ b.state != "running" })

        // (c) A text view keeps its Esc.
        let c = try heldJob(document, showProgress: true)
        let text = NSTextView(frame: NSRect(x: 0, y: 0, width: 80, height: 20))
        window.contentView?.addSubview(text)
        try require(window.makeFirstResponder(text) && window.firstResponder === text,
                    "[\(name)] (c) text view first responder")
        text.keyDown(with: escape)
        window.keyDown(with: escape)
        try require(c.state == "running" && !c.control.isCancelled,
                    "[\(name)] (c) Esc in a text view left the UI job running")
        window.makeFirstResponder(nil)
        text.removeFromSuperview()
        try drain(c)

        // (d) An agent's job is not the user's.
        let d = try heldJob(document, showProgress: false)
        window.makeFirstResponder(nil)
        window.keyDown(with: escape)
        try require(!d.control.isCancelled, "[\(name)] (d) Esc left the MCP job running")
        try drain(d)
    }
    history.close()
    snapshots.close()
    comps.close()
    print("PASS: floating window escape — Esc from History, Snapshots, Layer Comps, Actions and "
        + "the editor window (window or table/controller first responder) cancels the running UI "
        + "job, and leaves a text view's Esc and an MCP job alone")
}

func verifySilentRefusalBeeps() throws {
    let document = try fixture(width: 64, height: 64)
    defer { document.close() }
    AutomationAlerts.reset()
    let presented = document.presentError(SilentEditRefusal(message: "p"))
    try require(!presented && AutomationAlerts.recorded.isEmpty,
                "A SilentEditRefusal beeps and records nothing: \(AutomationAlerts.recorded)")
    document.presentError(DoctoredCoreError(message: "Probe error."))
    try require(AutomationAlerts.recorded.count == 1
                && AutomationAlerts.recorded.first?.kind == .errorModal,
                "Any other error is presented once: \(AutomationAlerts.recorded)")
    AutomationAlerts.reset()
    print("PASS: silent refusal beeps — a SilentEditRefusal presented through its document beeps "
        + "with no alert, and any other error still alerts")
}

func verifyGroupOpenNoUndo() throws {
    for depth in [8, 16] {
        let document = try depth == 8 ? fixture(width: 64, height: 64)
            : precisionFixture(width: 64, height: 64)
        defer { document.close() }
        _ = try editor(document)
        let grouped = try agentOK("group_layers", document, ["layers": [0]])
        guard let g = grouped["group"] as? Int, let open = document.doc?.layerInfo(g)?.open else {
            throw ProbeFailure(description: "[\(depth)] group_layers returned no group: \(grouped)")
        }
        document.updateChangeCount(.changeCleared)
        try require(!document.isDocumentEdited, "[\(depth)] clean after grouping")
        func retained() -> AnyObject? {
            depth == 8 ? document.retainedByteProjection : document.retainedPrecisionProjection
        }
        try pumpUntil({ retained() != nil }, seconds: 20)
        guard let projection = retained() else { throw ProbeFailure(description: "[\(depth)] projection") }
        let cursor = document.history.cursor, count = document.history.entries.count

        // MCP open: no undo step, dirtying, the projection kept.
        let reply = try agentOK("set_layer_properties", document, ["index": g, "open": !open])
        try require(reply["changed"] as? Bool != false, "[\(depth)] open changed: \(reply)")
        try require(document.history.cursor == cursor && document.history.entries.count == count,
                    "[\(depth)] MCP open added no history entry")
        try require(document.doc?.layerInfo(g)?.open == !open, "[\(depth)] MCP open flipped the disclosure")
        try require(document.isDocumentEdited, "[\(depth)] MCP open marks the document edited")
        try require(retained() === projection, "[\(depth)] MCP open kept the retained projection")
        let again = try agentOK("set_layer_properties", document, ["index": g, "open": !open])
        try require(again["changed"] as? Bool == false, "[\(depth)] repeating open answers changed:false: \(again)")

        // The triangle gives the same counts.
        document.setGroupExpanded(g, open, record: .notACommand)
        try require(document.doc?.layerInfo(g)?.open == open
                    && document.history.cursor == cursor && document.history.entries.count == count,
                    "[\(depth)] the triangle adds no history entry either")

        // {name, open}: exactly one entry, undone and redone together.
        guard let before = document.doc?.layerInfo(g)?.name else {
            throw ProbeFailure(description: "[\(depth)] group name")
        }
        _ = try agentOK("set_layer_properties", document, ["index": g, "name": "G2", "open": !open])
        try require(document.history.entries.count == count + 1 && document.history.cursor == cursor + 1,
                    "[\(depth)] name + open is exactly one entry")
        try require(document.doc?.layerInfo(g)?.name == "G2" && document.doc?.layerInfo(g)?.open == !open,
                    "[\(depth)] name + open both landed")
        document.undoManager?.undo()
        try require(document.doc?.layerInfo(g)?.name == before && document.doc?.layerInfo(g)?.open == open,
                    "[\(depth)] one undo reverts the name and the disclosure")
        document.undoManager?.redo()
        try require(document.doc?.layerInfo(g)?.name == "G2" && document.doc?.layerInfo(g)?.open == !open,
                    "[\(depth)] redo restores both")
    }
    print("PASS: group open no undo — MCP set_layer_properties open adds no history entry, dirties "
        + "and keeps the projection at 8 and 16 bits like the triangle, and with a name is one step")
}

func verifyGuidesRefusedMidGesture() throws {
    let document = try fixture(width: 64, height: 64)
    defer { document.close() }
    _ = try editor(document)
    guard let base = document.doc else { throw ProbeFailure(description: "Guides fixture") }
    let arguments: [String: Any] = ["document_id": AgentServer.shared.documentID(for: document),
                                    "orientation": "vertical", "position": 10]

    // A live gesture holds the document: the handler (past the dispatcher's
    // own live-gesture refusal) answers an error and leaves nothing behind.
    document.beginLiveEdit()
    try require(document.isLiveEditing, "live gesture open")
    do {
        _ = try AgentServer.shared.addGuide(arguments)
        document.abandonLiveEdit(ownedBy: base)
        throw ProbeFailure(description: "add_guide answered ok mid-gesture")
    } catch let error as AgentServer.ToolError {
        try require(error.message.contains("Guides are being edited on the canvas"),
                    "add_guide refuses mid-gesture by name: \(error.message)")
    }
    document.abandonLiveEdit(ownedBy: base)
    try require(document.doc === base && document.doc?.guideCount == 0
                && document.undoManager?.canUndo != true,
                "the refused guide edit changed nothing and left no undo step")

    // With no gesture the same call answers ok with one undo step.
    _ = try agentOK("add_guide", document, ["orientation": "vertical", "position": 10])
    try require(document.doc?.guideCount == 1 && document.undoManager?.canUndo == true,
                "add_guide adds the guide as one undo step")
    document.undoManager?.undo()
    try require(document.doc?.guideCount == 0 && document.undoManager?.canUndo != true,
                "one undo removes it")
    print("PASS: guides refused mid-gesture — an agent guide edit under a live gesture answers an "
        + "error with no undo step, and answers ok with one step otherwise")
}

// MARK: - Panel keys

func verifyPanelTableKeys() throws {
    let document = try fixture(width: 200, height: 160)
    defer { document.close() }
    let controller = try editor(document)
    guard let window = document.windowControllers.first?.window as? EditorWindow,
          let v = key("v", code: 9), let escape = escapeKey(),
          let down = key("\u{F701}", code: 125), let space = key(" ", code: 49),
          let commandV = key("v", code: 9, modifiers: [.command]) else {
        throw ProbeFailure(description: "Panel table keys fixture")
    }
    let canvas: ImageCanvasView = controller.canvas
    let saved = controller.captureWorkspace(name: "")
    defer { controller.applyPanelLayout(visible: saved.panelVisible, tab: saved.panelTab) }

    for tab in [EditorPanelTab.layers, .channels] {
        let name = tab.name
        controller.applyPanelLayout(visible: true, tab: tab)
        let found: NSTableView? = tab == .layers ? controller.layersPanel.tableView
            : firstTable(controller.channelsPanel.view)
        guard let table = found else { throw ProbeFailure(description: "[\(name)] panel table") }
        try require(table is CanvasKeyTableView, "[\(name)] the table is a CanvasKeyTableView")

        // (0) As the app builds it the table refuses first responder, so a
        // click leaves focus where it was; a key sent to it anyway reaches
        // the canvas.
        let refuses = table.refusesFirstResponder
        try require(refuses && !table.acceptsFirstResponder,
                    "[\(name)] (0) the table as built refuses first responder")
        _ = window.makeFirstResponder(table)
        try require(window.firstResponder !== table,
                    "[\(name)] (0) the table as built cannot take focus")
        controller.selectTool(.select)
        window.makeFirstResponder(nil)
        table.keyDown(with: v)
        try require(controller.currentTool == .move && window.firstResponder === canvas,
                    "[\(name)] (0) V sent to the unfocused table selected Move: "
                    + "\(controller.currentTool)")

        // A focused table: the probe lifts the refusal to put
        // the table itself in focus, and restores it.
        table.refusesFirstResponder = false
        defer { table.refusesFirstResponder = refuses }
        func focusTable(_ step: String) throws {
            try require(window.makeFirstResponder(table) && window.firstResponder === table,
                        "[\(name)] \(step) table first responder")
        }

        // (a) A bare tool key goes to the canvas.
        controller.selectTool(.select)
        try focusTable("(a)")
        table.keyDown(with: v)
        try require(controller.currentTool == .move,
                    "[\(name)] (a) V selected Move: \(controller.currentTool)")
        try require(window.firstResponder === canvas,
                    "[\(name)] (a) the canvas is first responder again")

        // (b) Esc with a selection deselects, as on the canvas.
        controller.selectTool(.select)
        _ = try select(document, side: 40)
        try focusTable("(b)")
        table.keyDown(with: escape)
        try require(document.canvasSelection == nil, "[\(name)] (b) Esc from the table deselected")

        // (c) Esc over a running UI job cancels it and keeps the selection.
        _ = try select(document, side: 40)
        let job = try heldJob(document, showProgress: true)
        try focusTable("(c)")
        table.keyDown(with: escape)
        try require(job.control.isCancelled && document.canvasSelection != nil,
                    "[\(name)] (c) Esc from the table cancelled the UI job and kept the selection")
        try pumpUntil({ job.state != "running" })
        document.setCanvasSelection(nil)
        document.undoManager?.removeAllActions()

        // (d) The table keeps its own keys and every modified key.
        controller.selectTool(.select)
        for (label, event) in [("Down", down), ("Space", space), ("⌘V", commandV)] {
            try focusTable("(d) \(label)")
            table.keyDown(with: event)
            try require(window.firstResponder === table && controller.currentTool == .select,
                        "[\(name)] (d) \(label) stayed in the table: \(controller.currentTool)")
        }
        try require(document.undoManager?.canUndo != true, "[\(name)] no key recorded a step")
    }
    print("PASS: panel table keys — from the Layers and Channels tables a bare tool key and Esc "
        + "reach the canvas (Esc deselects, or cancels the running UI job), while the arrows, "
        + "Space and a ⌘ key stay in the table")
}

func verifyTextSessionWindowKeys() throws {
    let document = try fixture(width: 200, height: 160)
    defer { document.close() }
    let controller = try editor(document)
    guard let window = document.windowControllers.first?.window as? EditorWindow,
          let v = key("v", code: 9), let b = key("b", code: 11) else {
        throw ProbeFailure(description: "Text session keys fixture")
    }
    let canvas: ImageCanvasView = controller.canvas
    let saved = controller.captureWorkspace(name: "")
    defer { controller.applyPanelLayout(visible: saved.panelVisible, tab: saved.panelTab) }
    controller.applyPanelLayout(visible: true, tab: .layers)
    controller.selectTool(.select)
    canvas.beginTextSession(at: CGPoint(x: 20, y: 20))
    defer { canvas.cancelTextSession() }
    guard let text = canvas.activeTextView else {
        throw ProbeFailure(description: "text session open")
    }

    // (a) The window itself first responder: the key goes to the text view.
    window.makeFirstResponder(nil)
    try require(window.firstResponder === window, "(a) the window is first responder")
    window.keyDown(with: v)
    try require(window.firstResponder === text, "(a) the text view is first responder again")
    try require(controller.currentTool == .select && canvas.activeTextView === text,
                "(a) V switched no tool and kept the session: \(controller.currentTool)")
    try require(text.string == "v", "(a) V was typed into the session: \"\(text.string)\"")

    // (b) From the Layers table: the same. The table refuses first responder
    // as the app builds it; the probe lifts that to put it in focus.
    let table = controller.layersPanel.tableView
    let refuses = table.refusesFirstResponder
    table.refusesFirstResponder = false
    defer { table.refusesFirstResponder = refuses }
    try require(window.makeFirstResponder(table) && window.firstResponder === table,
                "(b) table first responder")
    table.keyDown(with: b)
    try require(window.firstResponder === text, "(b) the text view is first responder again")
    try require(controller.currentTool == .select && canvas.activeTextView === text,
                "(b) B switched no tool and kept the session: \(controller.currentTool)")
    try require(text.string == "vb", "(b) B was typed into the session: \"\(text.string)\"")
    print("PASS: text session window keys — with a text session open, a key that reaches the "
        + "editor window or the Layers table goes to the session's text view, which takes focus "
        + "again, and switches no tool")
}

func verifyFloatingPanelUndo() throws {
    let document = try fixture(width: 64, height: 64)
    defer { document.close() }
    _ = try editor(document)
    guard let base = document.doc else { throw ProbeFailure(description: "Panel undo fixture") }
    let history = HistoryWindowController(document: document)
    let snapshots = SnapshotsWindowController(document: document)
    let comps = LayerCompsWindowController(document: document)
    defer {
        history.close()
        snapshots.close()
        comps.close()
    }
    let undoSelector = #selector(JobEscapePanel.undo(_:))
    let redoSelector = #selector(JobEscapePanel.redo(_:))
    for (name, windowController) in [("History", history as NSWindowController),
                                     ("Snapshots", snapshots), ("Layer Comps", comps)] {
        guard let panel = windowController.window as? JobEscapePanel,
              let content = panel.contentView, let table = firstTable(content) else {
            throw ProbeFailure(description: "[\(name)] panel and its table")
        }
        let undoItem = NSMenuItem(title: "Undo", action: undoSelector, keyEquivalent: "z")
        let redoItem = NSMenuItem(title: "Redo", action: redoSelector, keyEquivalent: "Z")
        try require(panel.makeFirstResponder(table) && panel.firstResponder === table,
                    "[\(name)] table first responder")

        // (a) Nothing to undo: both items are disabled, as in the editor.
        try require(!panel.validateUserInterfaceItem(undoItem)
                    && !panel.validateUserInterfaceItem(redoItem),
                    "[\(name)] (a) Undo and Redo are disabled on a fresh document")

        // (b) ⌘Z/⇧⌘Z through the table's responder chain move the document.
        let renamed = try rename(document)
        try require(panel.validateUserInterfaceItem(undoItem), "[\(name)] (b) Undo validates")
        try require(table.tryToPerform(undoSelector, with: nil) && document.doc === base,
                    "[\(name)] (b) ⌘Z from the table undid the rename")
        try require(panel.validateUserInterfaceItem(redoItem)
                    && !panel.validateUserInterfaceItem(undoItem),
                    "[\(name)] (b) Redo validates and Undo does not")
        try require(table.tryToPerform(redoSelector, with: nil) && document.doc === renamed,
                    "[\(name)] (b) ⇧⌘Z from the table redid the rename")

        // (c) Over a running UI job Undo reads "Cancel <title>" and cancels.
        let job = try heldJob(document, showProgress: true)
        try require(panel.validateUserInterfaceItem(undoItem)
                    && undoItem.title == "Cancel Held Probe",
                    "[\(name)] (c) Undo validates as Cancel Held Probe: \(undoItem.title)")
        try require(table.tryToPerform(undoSelector, with: nil) && job.control.isCancelled
                    && document.doc === renamed,
                    "[\(name)] (c) ⌘Z cancelled the job and popped nothing")
        try pumpUntil({ job.state != "running" })

        // (d) A text view in the panel keeps its own undo.
        let text = NSTextView(frame: NSRect(x: 0, y: 0, width: 80, height: 20))
        content.addSubview(text)
        try require(panel.makeFirstResponder(text) && panel.firstResponder === text,
                    "[\(name)] (d) text view first responder")
        panel.undo(nil)
        try require(document.doc === renamed, "[\(name)] (d) ⌘Z in a text view left the document")
        panel.makeFirstResponder(nil)
        text.removeFromSuperview()

        panel.undo(nil)
        try require(document.doc === base, "[\(name)] the last ⌘Z undid the rename")
        document.undoManager?.removeAllActions()
        document.history.reset(document.captureEditingState())
    }
    print("PASS: floating panel undo — ⌘Z/⇧⌘Z from History, Snapshots and Layer Comps undo and "
        + "redo the document, validate like the editor's, cancel its running UI job, and leave a "
        + "text view's own undo alone")
}

/// The first visible button of `window` whose action is the progress panel's
/// own Cancel.
private func progressCancel(_ window: NSWindow) -> NSButton? {
    window.contentView.map(buttons)?.first { $0.action == NSSelectorFromString("cancelClicked:") }
}

func verifyProgressPanelEscape() throws {
    let document = try fixture(width: 64, height: 64)
    defer { document.close() }
    _ = try editor(document)
    guard let base = document.doc, let escape = escapeKey() else {
        throw ProbeFailure(description: "Progress panel fixture")
    }

    // (a) A nonmodal job's panel: Cancel's key equivalent is Esc, and the
    // panel's own key-equivalent dispatch presses it.
    let job = try heldJob(document, showProgress: true)
    try pumpUntil({ job.panel != nil }, seconds: 5)
    guard let panel = job.panel?.window, let cancel = progressCancel(panel) else {
        throw ProbeFailure(description: "(a) the job's panel and its Cancel button")
    }
    try require(!cancel.isHidden && cancel.keyEquivalent == "\u{1b}"
                && cancel.keyEquivalentModifierMask.intersection([.command, .option, .control]).isEmpty,
                "(a) Cancel's key equivalent is bare Esc: \(cancel.keyEquivalent.unicodeScalars.map(\.value))")
    try require(panel.performKeyEquivalent(with: escape) && job.control.isCancelled,
                "(a) Esc in the panel pressed Cancel")
    try pumpUntil({ job.state != "running" })
    try require(job.state == "cancelled" && document.doc === base
                && document.undoManager?.canUndo != true,
                "(a) the job ends cancelled with nothing applied: \(job.report)")

    // (b) A wait that cannot be cancelled has no Cancel and no key equivalent;
    // (c) the same pooled panel, cancellable again, has Esc back.
    func waitPanelCancel(_ title: String, cancellable: Bool) throws -> (hidden: Bool, key: String)? {
        let control = cancellable ? try RasterJobControl() : nil
        let task = try WorkerTask<Bool> {
            let deadline = Date().addingTimeInterval(0.15)
            while Date() < deadline && control?.isCancelled != true {
                Thread.sleep(forTimeInterval: 0.005)
            }
            return true
        }
        var seen: (hidden: Bool, key: String)?
        let timer = Timer(timeInterval: 0.01, repeats: true) { _ in
            guard seen == nil, let modal = NSApp.modalWindow, modal.title == title,
                  let button = progressCancel(modal) else { return }
            seen = (button.isHidden, button.keyEquivalent)
        }
        RunLoop.main.add(timer, forMode: .modalPanel)
        defer { timer.invalidate() }
        _ = try BackgroundWork.wait(task, title: title, control: control)
        return seen
    }
    guard let fixed = try waitPanelCancel("Escape Probe B", cancellable: false),
          let again = try waitPanelCancel("Escape Probe C", cancellable: true) else {
        throw ProbeFailure(description: "(b) no modal progress panel was captured")
    }
    try require(fixed.hidden && fixed.key.isEmpty,
                "(b) a wait with no Cancel has no key equivalent: \(fixed)")
    try require(!again.hidden && again.key == "\u{1b}",
                "(c) a cancellable wait on the reused panel has Esc again: \(again)")
    try require(!BackgroundWork.occupied && BackgroundWork.scratchEstimate == 0,
                "the worker slot was released")
    print("PASS: progress panel escape — a cancellable progress panel's Cancel takes Esc as its "
        + "key equivalent and Esc in the panel cancels the job; a wait with no Cancel has none")
}
