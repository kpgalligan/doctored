import AppKit

// Edit ▸ Perspective Warp through the editor's own action and synthesized canvas events:
// two quads drawn in Layout, the second's corners released on the first's so they join along an
// edge, Warp, a pin dragged, an edge Shift-clicked to straighten, Return commits ONE
// "Perspective Warp" step recorded as one `perspective_warp` step and undone back to the base;
// Esc after a drag leaves no step; and an edit published from outside the session closes it with
// no Perspective Warp step.

/// An 8-bit document whose one layer is an opaque texture.
private func perspectiveFixture() throws -> ImageDocument {
    let (width, height) = (240, 180)
    var bytes = [UInt8](repeating: 0, count: width * height * 4)
    for y in 0..<height {
        for x in 0..<width {
            let p = (y * width + x) * 4
            bytes[p] = UInt8(40 + (x * 3) % 170)
            bytes[p + 1] = UInt8(40 + (y * 5) % 170)
            bytes[p + 2] = UInt8((x + y) % 256)
            bytes[p + 3] = 255
        }
    }
    guard let image = RasterImage.from(rgba: bytes, width: width, height: height),
          let document = ImageDocument.makeUntitled(with: image) else {
        throw ProbeFailure(description: "[perspective warp] fixture allocation")
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

private func perspectiveMouse(_ canvas: ImageCanvasView, _ type: NSEvent.EventType, _ point: CGPoint,
                              _ modifiers: NSEvent.ModifierFlags = []) throws -> NSEvent {
    guard let window = canvas.window, let event = NSEvent.mouseEvent(
        with: type, location: canvas.convert(point, to: nil), modifierFlags: modifiers,
        timestamp: ProcessInfo.processInfo.systemUptime, windowNumber: window.windowNumber,
        context: nil, eventNumber: 0, clickCount: 1, pressure: 1) else {
        throw ProbeFailure(description: "[perspective warp] could not make a \(type.rawValue) event")
    }
    return event
}

private func perspectiveKey(_ canvas: ImageCanvasView, _ code: UInt16, _ characters: String) throws -> NSEvent {
    guard let window = canvas.window, let event = NSEvent.keyEvent(
        with: .keyDown, location: .zero, modifierFlags: [],
        timestamp: ProcessInfo.processInfo.systemUptime, windowNumber: window.windowNumber,
        context: nil, characters: characters, charactersIgnoringModifiers: characters,
        isARepeat: false, keyCode: code) else {
        throw ProbeFailure(description: "[perspective warp] could not make key \(code)")
    }
    return event
}

/// A press at `a`, a drag to `b`, a release there.
private func perspectiveDrag(_ canvas: ImageCanvasView, _ a: CGPoint, _ b: CGPoint,
                             _ modifiers: NSEvent.ModifierFlags = []) throws {
    canvas.mouseDown(with: try perspectiveMouse(canvas, .leftMouseDown, a, modifiers))
    canvas.mouseDragged(with: try perspectiveMouse(canvas, .leftMouseDragged, b, modifiers))
    canvas.mouseUp(with: try perspectiveMouse(canvas, .leftMouseUp, b, modifiers))
}

/// Opens the session, draws two quads that join along an edge, switches to
/// Warp, drags quad 0's first pin and Shift-clicks quad 1's right edge.
private func perspectiveLayoutAndWarp(_ controller: EditorViewController, _ tag: String) throws {
    let canvas: ImageCanvasView = controller.canvas
    controller.showPerspectiveWarp(nil)
    try require(canvas.perspectiveWarpSession != nil,
                "\(tag) Edit ▸ Perspective Warp opened the session: \(uiAlerts())")
    try perspectiveDrag(canvas, CGPoint(x: 40, y: 40), CGPoint(x: 110, y: 120))
    // Drawn from its far corner, so the press is not on quad 0's corner;
    // its near corners land 2 px from quad 0's right corners and join them.
    try perspectiveDrag(canvas, CGPoint(x: 180, y: 121), CGPoint(x: 112, y: 41))
    guard var session = canvas.perspectiveWarpSession else {
        throw ProbeFailure(description: "\(tag) the session closed in Layout")
    }
    try require(session.layout.quads.count == 2 && session.layout.sharedEdges.count == 1
                && session.layout.vertices.count == 6,
                "\(tag) two quads joined along one edge: \(session.layout.quads) "
                + "shared \(session.layout.sharedEdges)")
    try session.setMode(.warp)
    canvas.perspectiveWarpSession = session
    try perspectiveDrag(canvas, CGPoint(x: 40, y: 40), CGPoint(x: 50, y: 47))
    canvas.mouseDown(with: try perspectiveMouse(canvas, .leftMouseDown, CGPoint(x: 180, y: 80), .shift))
    canvas.mouseUp(with: try perspectiveMouse(canvas, .leftMouseUp, CGPoint(x: 180, y: 80), .shift))
    guard let warped = canvas.perspectiveWarpSession else {
        throw ProbeFailure(description: "\(tag) the session closed in Warp")
    }
    try require(warped.layout.vertices[0].to != warped.layout.vertices[0].from
                && warped.preview != nil && !warped.isIdentity,
                "\(tag) the dragged pin moved and the proxy re-rendered")
    try require(warped.layout.straighten.count == 1
                && warped.layout.straighten[0].direction == .vertical,
                "\(tag) the Shift-clicked edge straightened vertical: \(warped.layout.straighten)")
}

func verifyPerspectiveWarp() throws {
    let tag = "[perspective warp]"
    let document = try perspectiveFixture()
    defer {
        if ActionRecorder.shared.isRecording { _ = ActionRecorder.shared.stop() }
        document.close()
    }
    let controller = try editor(document)
    let canvas: ImageCanvasView = controller.canvas
    guard let base = document.doc else { throw ProbeFailure(description: "\(tag) no base") }
    let entry = SelectionWorkBudget.scratchBytes
    let entries = document.history.entries.count

    // Two joined quads, a pin dragged, an edge straightened, Return: one
    // step, one recorded step, one undo.
    let steps = try uiRecorded {
        try perspectiveLayoutAndWarp(controller, tag)
        canvas.keyDown(with: try perspectiveKey(canvas, 36, "\r"))
        try pumpUntil({ !BackgroundWork.occupied && DocumentJobs.active == nil }, seconds: 60)
    }
    try require(canvas.perspectiveWarpSession == nil, "\(tag) Return closed the session: \(uiAlerts())")
    guard let warped = document.doc, warped !== base else {
        throw ProbeFailure(description: "\(tag) Return did not change the layer: \(uiAlerts())")
    }
    try require(document.undoManager?.undoActionName == PerspectiveWarp.actionName
                && document.history.entries.count == entries + 1,
                "\(tag) one \"Perspective Warp\" step: \(document.history.entries.map(\.name))")
    try require(steps.count == 1 && steps[0].tool == "perspective_warp"
                && (steps[0].arguments["quads"] as? [Any])?.count == 2,
                "\(tag) exactly one recorded perspective_warp step with two quads: \(steps.map(\.tool))")
    document.undoManager?.undo()
    try require(document.doc === base, "\(tag) one undo restores the layer")
    try pumpUntil({ SelectionWorkBudget.scratchBytes <= entry }, seconds: 10)

    // Esc after a drag: no step.
    let revision = document.revision
    try perspectiveLayoutAndWarp(controller, "\(tag) esc")
    canvas.keyDown(with: try perspectiveKey(canvas, 53, "\u{1B}"))
    try require(canvas.perspectiveWarpSession == nil && document.doc === base
                && document.revision == revision,
                "\(tag) Esc closes the session and changes nothing")
    try pumpUntil({ SelectionWorkBudget.scratchBytes <= entry }, seconds: 10)

    // An edit published from outside the session closes it with no step.
    try perspectiveLayoutAndWarp(controller, "\(tag) outside")
    withUndoGroup(document.undoManager) {
        document.applyEdit("Rename", record: .notACommand) { try $0.withLayerName(0, "Changed") }
    }
    try require(canvas.perspectiveWarpSession == nil, "\(tag) an outside edit closed the session")
    try require(document.undoManager?.undoActionName == "Rename"
                && document.history.entries.last?.name == "Rename",
                "\(tag) the outside edit is the last step, with no Perspective Warp: "
                + "\(document.history.entries.map(\.name))")
    try pumpUntil({ SelectionWorkBudget.scratchBytes <= entry }, seconds: 10)
    print("PASS: perspective warp — Edit ▸ Perspective Warp with two quads joined in Layout, a pin "
        + "dragged and an edge straightened in Warp commits on Return as one \"Perspective Warp\" "
        + "step and one recorded perspective_warp step that one undo restores; Esc after a drag "
        + "leaves no step; an outside edit closes the session with no step")
}
