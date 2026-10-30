import AppKit

// Edit ▸ Puppet Warp through the editor's own action and synthesized canvas events:
// two pins added, one dragged, Return commits ONE "Puppet Warp" step recorded as one
// `puppet_warp` step and undone back to the base; Esc after a drag leaves no step; and an edit
// published from outside the session closes it with no Puppet Warp step.

/// An 8-bit document whose one layer is an opaque ellipse on transparency.
private func puppetFixture() throws -> ImageDocument {
    let (width, height) = (240, 180)
    var bytes = [UInt8](repeating: 0, count: width * height * 4)
    for y in 0..<height {
        for x in 0..<width {
            let dx = (Double(x) + 0.5 - 120) / 70, dy = (Double(y) + 0.5 - 90) / 50
            guard dx * dx + dy * dy <= 1 else { continue }
            let p = (y * width + x) * 4
            bytes[p] = UInt8(40 + (x * 3) % 170)
            bytes[p + 1] = UInt8(40 + (y * 5) % 170)
            bytes[p + 2] = 120
            bytes[p + 3] = 255
        }
    }
    guard let image = RasterImage.from(rgba: bytes, width: width, height: height),
          let document = ImageDocument.makeUntitled(with: image) else {
        throw ProbeFailure(description: "[puppet warp] fixture allocation")
    }
    document.undoManager?.groupsByEvent = false
    document.updateChangeCount(.changeCleared)
    NSDocumentController.shared.addDocument(document)
    document.makeWindowControllers()
    if let window = document.windowControllers.first?.window {
        window.setFrameOrigin(NSPoint(x: -10000, y: -10000))
        window.orderFront(nil)
    }
    document.undoManager?.removeAllActions()
    return document
}

private func puppetMouse(_ canvas: ImageCanvasView, _ type: NSEvent.EventType,
                         _ point: CGPoint) throws -> NSEvent {
    guard let window = canvas.window, let event = NSEvent.mouseEvent(
        with: type, location: canvas.convert(point, to: nil), modifierFlags: [],
        timestamp: ProcessInfo.processInfo.systemUptime, windowNumber: window.windowNumber,
        context: nil, eventNumber: 0, clickCount: 1, pressure: 1) else {
        throw ProbeFailure(description: "[puppet warp] could not make a \(type.rawValue) event")
    }
    return event
}

private func puppetKey(_ canvas: ImageCanvasView, _ code: UInt16, _ characters: String) throws -> NSEvent {
    guard let window = canvas.window, let event = NSEvent.keyEvent(
        with: .keyDown, location: .zero, modifierFlags: [],
        timestamp: ProcessInfo.processInfo.systemUptime, windowNumber: window.windowNumber,
        context: nil, characters: characters, charactersIgnoringModifiers: characters,
        isARepeat: false, keyCode: code) else {
        throw ProbeFailure(description: "[puppet warp] could not make key \(code)")
    }
    return event
}

/// Opens the session, clicks a pin at `a`, presses at `b` and drags it to `to`.
private func puppetPinAndDrag(_ controller: EditorViewController, a: CGPoint, b: CGPoint,
                              to: CGPoint, _ tag: String) throws {
    let canvas: ImageCanvasView = controller.canvas
    controller.showPuppetWarp(nil)
    try require(canvas.puppetWarpSession != nil, "\(tag) Edit ▸ Puppet Warp opened the session: \(uiAlerts())")
    canvas.mouseDown(with: try puppetMouse(canvas, .leftMouseDown, a))
    canvas.mouseUp(with: try puppetMouse(canvas, .leftMouseUp, a))
    canvas.mouseDown(with: try puppetMouse(canvas, .leftMouseDown, b))
    canvas.mouseDragged(with: try puppetMouse(canvas, .leftMouseDragged, to))
    canvas.mouseUp(with: try puppetMouse(canvas, .leftMouseUp, to))
    guard let session = canvas.puppetWarpSession else {
        throw ProbeFailure(description: "\(tag) the session closed under the pins")
    }
    try require(session.pins.count == 2, "\(tag) two pins: \(session.pins.count)")
    try require(session.pins[1].to != session.pins[1].from && session.preview != nil,
                "\(tag) the dragged pin moved and the proxy re-rendered")
}

func verifyPuppetWarp() throws {
    let tag = "[puppet warp]"
    let document = try puppetFixture()
    defer {
        if ActionRecorder.shared.isRecording { _ = ActionRecorder.shared.stop() }
        document.close()
    }
    let controller = try editor(document)
    let canvas: ImageCanvasView = controller.canvas
    guard let base = document.doc else { throw ProbeFailure(description: "\(tag) no base") }
    let entry = SelectionWorkBudget.scratchBytes
    let entries = document.history.entries.count
    let a = CGPoint(x: 90, y: 90), b = CGPoint(x: 150, y: 90), to = CGPoint(x: 170, y: 100)

    // Two pins, one dragged, Return: one step, one recorded step, one undo.
    let steps = try uiRecorded {
        try puppetPinAndDrag(controller, a: a, b: b, to: to, tag)
        canvas.keyDown(with: try puppetKey(canvas, 36, "\r"))
        try pumpUntil({ !BackgroundWork.occupied && DocumentJobs.active == nil }, seconds: 60)
    }
    try require(canvas.puppetWarpSession == nil, "\(tag) Return closed the session: \(uiAlerts())")
    guard let warped = document.doc, warped !== base else {
        throw ProbeFailure(description: "\(tag) Return did not change the layer: \(uiAlerts())")
    }
    try require(document.undoManager?.undoActionName == PuppetWarp.actionName
                && document.history.entries.count == entries + 1,
                "\(tag) one \"Puppet Warp\" step: \(document.history.entries.map(\.name))")
    try require(steps.count == 1 && steps[0].tool == "puppet_warp"
                && (steps[0].arguments["pins"] as? [Any])?.count == 2,
                "\(tag) exactly one recorded puppet_warp step with two pins: \(steps.map(\.tool))")
    document.undoManager?.undo()
    try require(document.doc === base, "\(tag) one undo restores the layer")
    try pumpUntil({ SelectionWorkBudget.scratchBytes <= entry }, seconds: 10)

    // Esc after a drag: no step.
    let revision = document.revision
    try puppetPinAndDrag(controller, a: a, b: b, to: to, "\(tag) esc")
    canvas.keyDown(with: try puppetKey(canvas, 53, "\u{1B}"))
    try require(canvas.puppetWarpSession == nil && document.doc === base
                && document.revision == revision,
                "\(tag) Esc closes the session and changes nothing")
    try pumpUntil({ SelectionWorkBudget.scratchBytes <= entry }, seconds: 10)

    // An edit published from outside the session closes it with no step.
    try puppetPinAndDrag(controller, a: a, b: b, to: to, "\(tag) outside")
    withUndoGroup(document.undoManager) {
        document.applyEdit("Rename", record: .notACommand) { try $0.withLayerName(0, "Changed") }
    }
    try require(canvas.puppetWarpSession == nil, "\(tag) an outside edit closed the session")
    try require(document.undoManager?.undoActionName == "Rename"
                && document.history.entries.last?.name == "Rename",
                "\(tag) the outside edit is the last step, with no Puppet Warp: "
                + "\(document.history.entries.map(\.name))")
    try pumpUntil({ SelectionWorkBudget.scratchBytes <= entry }, seconds: 10)
    print("PASS: puppet warp — Edit ▸ Puppet Warp with two pins and a drag commits on Return as one "
        + "\"Puppet Warp\" step and one recorded puppet_warp step that one undo restores; Esc after "
        + "a drag leaves no step; an outside edit closes the session with no step")
}
