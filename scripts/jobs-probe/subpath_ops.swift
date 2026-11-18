import AppKit

// "path subpath operations" (`designs/v1-paths-held-subpaths.md`, HS-7,
// EditorViewController+SubpathOps.swift), run off screen: the Path Selection options bar's
// Operation popup on a list path, a vector mask and a path shape's outline. Each set is ONE
// history entry named "Set Subpath Operation" recording one `set_path` / `set_vector_mask` /
// `edit_shape_layer` whose `subpaths` all carry `op`, that undoes to the shape without operations
// and replays to the same shape; the selection survives it and the popup reads the operation
// back; the same operation again records nothing. On a stroked path shape the set records one
// alert and changes nothing; Combine Selected Subpaths on a holding outline leaves no operations;
// a holding payload says `"version":8` and a plain path payload re-encodes to its own text at
// version 7. `PathShape.foldCGPath` keeps one move per subpath where `cgPath` drops one, before
// and after a shearing `copy(using:)`; a holding outline given a one-anchor subpath by an
// ⌥-drag copy still renders, its pixels those of the outline without the copy; and Paths panel ▸
// Convert to Shape Layer of a holding path with a stroke in the options, and a holding path
// shape's re-edit session restyled with a stroke, each record one alert naming the stroke and
// change nothing.

/// Rectangle A, rectangle B overlapping it on the right, and a one-anchor subpath inside A.
private let opsRects: [[String: Any]] = [
    ["closed": true, "anchors": [[20, 20], [120, 20], [120, 100], [20, 100]]],
    ["closed": true, "anchors": [[70, 50], [170, 50], [170, 140], [70, 140]]],
]
private let opsDot: [String: Any] = ["closed": false, "anchors": [[50, 50]]]
/// A press inside B and outside A: takes subpath 1.
private let insideB = CGPoint(x: 150, y: 120)

private func opsEvent(
    _ canvas: ImageCanvasView, _ type: NSEvent.EventType, _ point: CGPoint,
    _ flags: NSEvent.ModifierFlags = []
) throws -> NSEvent {
    guard let window = canvas.window, let event = NSEvent.mouseEvent(
        with: type, location: canvas.convert(point, to: nil), modifierFlags: flags,
        timestamp: ProcessInfo.processInfo.systemUptime, windowNumber: window.windowNumber,
        context: nil, eventNumber: 1, clickCount: 1, pressure: 1) else {
        throw ProbeFailure(description: "no \(type) event at \(point)")
    }
    return event
}

private func opsDrag(
    _ canvas: ImageCanvasView, from: CGPoint, to: CGPoint, _ flags: NSEvent.ModifierFlags = []
) throws {
    canvas.mouseDown(with: try opsEvent(canvas, .leftMouseDown, from, flags))
    if from != to {
        let middle = CGPoint(x: (from.x + to.x) / 2, y: (from.y + to.y) / 2)
        canvas.mouseDragged(with: try opsEvent(canvas, .leftMouseDragged, middle, flags))
        canvas.mouseDragged(with: try opsEvent(canvas, .leftMouseDragged, to, flags))
    }
    canvas.mouseUp(with: try opsEvent(canvas, .leftMouseUp, to, flags))
}

/// The options bar's Operation popup: its rows, `get`, `set` and enablement.
private struct OperationPopup {
    let items: [String]
    let get: () -> Int
    let set: (Int) -> Void
    let enabled: () -> Bool
}

private func operationPopup(_ controller: EditorViewController) throws -> OperationPopup {
    for cluster in controller.pathSelectClusters() {
        for descriptor in cluster.descriptors where descriptor.id == "pathSelect.operation" {
            if case let .popup(_, items, get, set) = descriptor.kind {
                return OperationPopup(items: items, get: get, set: set,
                                      enabled: descriptor.isEnabled)
            }
        }
    }
    throw ProbeFailure(description: "no pathSelect.operation popup in the options bar")
}

private func opsCurrent(_ document: ImageDocument) -> PathShape? {
    document.resolvedPath(document.currentPath)
}

/// The popup set to `row` as ONE history entry recording one `tool` step whose subpaths all carry
/// `op`; the selection kept and the popup reading `row` back; the undo restoring `before` and the
/// step replaying to the same shape. Returns the shape it left.
private func requireOperationStep(
    _ document: ImageDocument, _ controller: EditorViewController, _ popup: OperationPopup,
    row: Int, tool: String, before: PathShape, _ tag: String
) throws -> PathShape {
    let entries = document.history.entries.count
    let selection = controller.canvas.pathSelectSession.selection
    let steps = uiRecorded { popup.set(row) }
    guard let after = opsCurrent(document) else {
        throw ProbeFailure(description: "\(tag) the current path is gone")
    }
    try require(document.history.entries.count == entries + 1
                && document.undoManager?.undoActionName == "Set Subpath Operation",
                "\(tag) one \"Set Subpath Operation\" step: \(document.history.entries.map(\.name))")
    let subpaths = steps.first?.arguments["subpaths"] as? [[String: Any]] ?? []
    try require(steps.count == 1 && steps[0].tool == tool && !subpaths.isEmpty
                && subpaths.allSatisfy({ $0["op"] is String }),
                "\(tag) one recorded \(tool) whose subpaths all carry op: "
                + "\(steps.map { "\($0.tool) \($0.arguments)" })")
    try require(steps[0].note == "Path Selection ▸ Operation",
                "\(tag) the note: \(steps[0].note ?? "none")")
    try require(after.ops != nil, "\(tag) the shape holds operations")
    try require(controller.canvas.pathSelectSession.selection == selection,
                "\(tag) the selection survives the set")
    try require(popup.get() == row, "\(tag) the popup reads \(row) back: \(popup.get())")
    // The same operation again records nothing.
    let again = uiRecorded { popup.set(row) }
    try require(again.isEmpty && document.history.entries.count == entries + 1,
                "\(tag) the same operation again records nothing: \(again.map(\.tool))")
    popup.set(0)
    try require(document.history.entries.count == entries + 1, "\(tag) — records nothing")
    document.undoManager?.undo()
    try require(opsCurrent(document).map { PathSelectSession.matches($0, before) } == true,
                "\(tag) one undo restores the shape exactly")
    let replay = ActionPlayer.runSingle(steps[0], on: document, undoName: "Replay")
    guard replay.ok, let replayed = opsCurrent(document) else {
        throw ProbeFailure(description: "\(tag) the replay failed: \(replay.failures)")
    }
    try require(PathSelectSession.matches(replayed, after),
                "\(tag) the recorded step replays to the same shape")
    return replayed
}

func verifyPathSubpathOperations() throws {
    var failures: [String] = []
    for part in [
        verifyOperationOnList, verifyOperationOnVectorMask, verifyOperationOnOutline,
        verifyFoldMoves, verifyOperationOptionCopy, verifyHeldOpsStrokeCommits,
    ] {
        do { try part() } catch { failures.append(String(describing: error)) }
    }
    try require(failures.isEmpty, failures.joined(separator: " ;; "))
    print("PASS: the Operation popup sets the selected subpaths' operation as one recorded "
          + "set_path, set_vector_mask or edit_shape_layer step; a stroked shape alerts; Combine "
          + "drops the operations; payload 8 and 7; foldCGPath keeps a move per subpath; an "
          + "⌥-copied one-anchor subpath still renders; Convert to Shape Layer and the shape "
          + "re-edit refuse a stroke on a holding path by name")
}

/// A document with `subpaths` as its work path, the current path, and Path Selection.
private func opsFixture(_ subpaths: [[String: Any]]) throws -> (ImageDocument, EditorViewController) {
    let document = try fixture(width: 200, height: 160)
    do {
        let controller = try editor(document)
        _ = try agentOK("set_path", document, ["subpaths": subpaths])
        guard let work = document.doc?.paths.workPath else {
            throw ProbeFailure(description: "no work path")
        }
        controller.setCurrentPath(.path(work.id))
        controller.selectTool(.pathSelect)
        document.undoManager?.removeAllActions()
        return (document, controller)
    } catch {
        document.close()
        throw error
    }
}

private func verifyOperationOnList() throws {
    let tag = "[subpath operation list]"
    let (document, controller) = try opsFixture(opsRects)
    defer {
        if ActionRecorder.shared.isRecording { _ = ActionRecorder.shared.stop() }
        document.close()
    }
    let popup = try operationPopup(controller)
    try require(popup.items == ["—", "Unite", "Subtract", "Intersect", "Exclude"],
                "\(tag) the rows: \(popup.items)")
    try require(!popup.enabled() && popup.get() == 0, "\(tag) off and — with nothing selected")
    try opsDrag(controller.canvas, from: insideB, to: insideB)
    try require(controller.canvas.pathSelectSession.selection.subpaths == [1],
                "\(tag) the press selects 1")
    try require(popup.enabled() && popup.get() == 0, "\(tag) on, and — on a path holding none")
    guard let base = opsCurrent(document) else { throw ProbeFailure(description: "\(tag) no path") }
    let after = try requireOperationStep(
        document, controller, popup, row: 2, tool: "set_path", before: base, tag)
    try require(after.ops == [.unite, .subtract], "\(tag) [unite, subtract]: \(String(describing: after.ops))")
    try require(document.doc?.paths.workPath?.shape.ops == [.unite, .subtract],
                "\(tag) the work path holds them")
}

private func verifyOperationOnVectorMask() throws {
    let tag = "[subpath operation vector mask]"
    let (document, controller) = try opsFixture(opsRects)
    defer {
        if ActionRecorder.shared.isRecording { _ = ActionRecorder.shared.stop() }
        document.close()
    }
    _ = try agentOK("set_vector_mask", document, ["layer": 0, "subpaths": opsRects])
    guard let layerID = document.doc?.layerInfo(0)?.id else { throw ProbeFailure(description: tag) }
    controller.setCurrentPath(.vectorMask(layerID: layerID))
    document.undoManager?.removeAllActions()
    let popup = try operationPopup(controller)
    try opsDrag(controller.canvas, from: insideB, to: insideB)
    guard let base = opsCurrent(document) else { throw ProbeFailure(description: "\(tag) no mask") }
    let after = try requireOperationStep(
        document, controller, popup, row: 3, tool: "set_vector_mask", before: base, tag)
    try require(document.doc?.vectorMask(0)?.shape.ops == [.unite, .intersect]
                && after.ops == [.unite, .intersect], "\(tag) the mask holds [unite, intersect]")
}

private func verifyOperationOnOutline() throws {
    let tag = "[subpath operation outline]"
    let document = try fixture(width: 200, height: 160)
    defer {
        if ActionRecorder.shared.isRecording { _ = ActionRecorder.shared.stop() }
        document.close()
    }
    let controller = try editor(document)
    let plain = try agentOK("add_shape_layer", document,
                            ["kind": "path", "subpaths": opsRects, "fill": "#000000"])
    guard let index = plain["layer"] as? Int, let layerID = document.doc?.layerInfo(index)?.id,
          let plainMeta = document.doc?.layerMeta(index)
    else { throw ProbeFailure(description: "\(tag) add_shape_layer: \(plain)") }
    // A plain path payload: version 7, and its own text again when re-encoded.
    try require(plainMeta.contains("\"version\":7")
                && ShapeLayerPayload.decode(plainMeta)?.json() == plainMeta,
                "\(tag) a plain path payload is version 7 and re-encodes to its own text")
    controller.setCurrentPath(.shapeOutline(layerID: layerID))
    controller.selectTool(.pathSelect)
    document.undoManager?.removeAllActions()
    let popup = try operationPopup(controller)
    try opsDrag(controller.canvas, from: insideB, to: insideB)
    guard let base = opsCurrent(document) else { throw ProbeFailure(description: "\(tag) no outline") }
    _ = try requireOperationStep(
        document, controller, popup, row: 2, tool: "edit_shape_layer", before: base, tag)
    guard let held = document.doc?.layerMeta(index) else { throw ProbeFailure(description: tag) }
    try require(held.contains("\"version\":8") && held.contains("\"op\":\"subtract\""),
                "\(tag) the holding payload is version 8: \(held.prefix(160))")

    // Combine Selected Subpaths on the holding outline: geometry, no operations, version 7.
    try opsDrag(controller.canvas, from: insideB, to: insideB)
    controller.commitCombineSelectedSubpaths(.unite)
    try require(opsCurrent(document)?.ops == nil
                && document.doc?.layerMeta(index)?.contains("\"version\":7") == true,
                "\(tag) Combine leaves an outline holding no operations")

    // A stroked path shape: the set alerts once and changes nothing.
    let stroked = try agentOK("add_shape_layer", document, [
        "kind": "path", "subpaths": opsRects, "fill": "#000000", "stroke": "#ff0000",
        "stroke_width": 3,
    ])
    guard let strokedIndex = stroked["layer"] as? Int,
          let strokedID = document.doc?.layerInfo(strokedIndex)?.id
    else { throw ProbeFailure(description: "\(tag) the stroked shape: \(stroked)") }
    controller.setCurrentPath(.shapeOutline(layerID: strokedID))
    try opsDrag(controller.canvas, from: insideB, to: insideB)
    let entries = document.history.entries.count
    let before = document.doc
    AutomationAlerts.reset()
    let steps = uiRecorded { popup.set(2) }
    let alerts = AutomationAlerts.recorded
    AutomationAlerts.reset()
    try require(alerts.count == 1 && alerts[0].informativeText.contains("cannot take a stroke"),
                "\(tag) one alert: \(alerts.map(\.informativeText))")
    try require(steps.isEmpty && document.history.entries.count == entries && document.doc === before,
                "\(tag) the stroked shape is unchanged")
}

/// `foldCGPath` of [triangle, one-anchor open, one-anchor closed, triangle, one-anchor open]
/// keeps 5 moves before and after a shearing copy; `cgPath` keeps 4.
private func verifyFoldMoves() throws {
    let tag = "[fold moves]"
    func triangle(_ x: CGFloat) -> PathSubpath {
        PathSubpath(anchors: [.corner(CGPoint(x: x, y: 10)), .corner(CGPoint(x: x + 20, y: 10)),
                              .corner(CGPoint(x: x + 10, y: 30))], closed: true)
    }
    func lone(_ x: CGFloat, closed: Bool) -> PathSubpath {
        PathSubpath(anchors: [.corner(CGPoint(x: x, y: 50))], closed: closed)
    }
    let shape = PathShape(subpaths: [triangle(0), lone(40, closed: false), lone(50, closed: true),
                                     triangle(60), lone(90, closed: false)])
    func moves(_ path: CGPath) -> Int {
        var count = 0
        path.applyWithBlock { if $0.pointee.type == .moveToPoint { count += 1 } }
        return count
    }
    var shear = CGAffineTransform(a: 1, b: 0.2, c: 0.4, d: 1, tx: 3, ty: 5)
    let fold = shape.foldCGPath()
    let sheared = fold.copy(using: &shear)
    try require(moves(fold) == 5 && sheared.map(moves) == 5,
                "\(tag) foldCGPath: \(moves(fold)) moves, \(sheared.map(moves) ?? -1) sheared")
    try require(moves(shape.cgPath()) == 4, "\(tag) cgPath: \(moves(shape.cgPath())) moves")
}

/// A holding outline [A unite, B subtract, one-anchor unite]: an ⌥-drag copy of the one-anchor
/// subpath (a unite) still renders, and the layer's pixels are the outline's without the copy.
private func verifyOperationOptionCopy() throws {
    let tag = "[subpath operation ⌥-copy]"
    let document = try fixture(width: 200, height: 160)
    defer {
        if ActionRecorder.shared.isRecording { _ = ActionRecorder.shared.stop() }
        document.close()
    }
    let controller = try editor(document)
    let made = try agentOK("add_shape_layer", document,
                           ["kind": "path", "subpaths": opsRects + [opsDot], "fill": "#000000"])
    guard let index = made["layer"] as? Int, let layerID = document.doc?.layerInfo(index)?.id
    else { throw ProbeFailure(description: "\(tag) add_shape_layer: \(made)") }
    controller.setCurrentPath(.shapeOutline(layerID: layerID))
    controller.selectTool(.pathSelect)
    document.undoManager?.removeAllActions()
    let popup = try operationPopup(controller)
    try opsDrag(controller.canvas, from: insideB, to: insideB)
    popup.set(2)
    guard let held = opsCurrent(document), held.ops == [.unite, .subtract, .unite] else {
        throw ProbeFailure(description: "\(tag) the popup's set: \(String(describing: opsCurrent(document)?.ops))")
    }
    let pixels = document.doc?.layerPixels(index)
    let info = document.doc?.layerInfo(index)
    // Press the one-anchor subpath, then ⌥-drag a copy of it to (60, 60), inside A.
    try opsDrag(controller.canvas, from: CGPoint(x: 50, y: 50), to: CGPoint(x: 50, y: 50))
    try require(controller.canvas.pathSelectSession.selection.subpaths == [2],
                "\(tag) the press takes the one-anchor subpath")
    let entries = document.history.entries.count
    try opsDrag(controller.canvas, from: CGPoint(x: 50, y: 50), to: CGPoint(x: 60, y: 60), .option)
    guard let copied = opsCurrent(document) else { throw ProbeFailure(description: "\(tag) gone") }
    try require(document.history.entries.count == entries + 1
                && copied.ops == [.unite, .subtract, .unite, .unite],
                "\(tag) one Duplicate Subpaths step, the copy a unite: \(String(describing: copied.ops)) "
                + "\(document.history.entries.map(\.name)) \(uiAlerts())")
    let now = document.doc?.layerInfo(index)
    try require(document.doc?.layerPixels(index) == pixels && now?.offsetX == info?.offsetX
                && now?.offsetY == info?.offsetY && now?.width == info?.width,
                "\(tag) the layer renders the outline without the copy")
}

/// The two other canvas commits of a stroke onto a holding path (HS-4): Paths panel ▸ Convert to
/// Shape Layer of a holding work path with a stroke in the shape options, and the re-edit session
/// of a holding path shape restyled with a stroke. Each records one alert naming the stroke
/// (`ShapeLayerPayload.heldOpsRefusal`, not the core's generic sentence), no step, no history
/// entry and no new document; the conversion adds no layer and the session stays open.
private func verifyHeldOpsStrokeCommits() throws {
    let tag = "[subpath operation stroke commits]"
    let held: [[String: Any]] = opsRects.enumerated().map { index, subpath in
        var subpath = subpath
        subpath["op"] = index == 0 ? "unite" : "subtract"
        return subpath
    }
    let (document, controller) = try opsFixture(held)
    let options = ToolOptionsStore.shared.shape
    defer {
        controller.cancelShapeEditSession()
        ToolOptionsStore.shared.shape = options
        if ActionRecorder.shared.isRecording { _ = ActionRecorder.shared.stop() }
        document.close()
    }
    let red = NSColor(srgbRed: 1, green: 0, blue: 0, alpha: 1)
    /// `body` refused by the held-ops stroke alert alone, with nothing recorded or changed.
    func requireStrokeRefused(_ what: String, _ body: () -> Void) throws {
        let entries = document.history.entries.count
        let before = document.doc
        let layers = before?.layerCount ?? 0
        AutomationAlerts.reset()
        let steps = uiRecorded(body)
        let alerts = AutomationAlerts.recorded
        AutomationAlerts.reset()
        try require(alerts.count == 1 && alerts[0].informativeText.contains("cannot take a stroke"),
                    "\(tag) \(what): one alert naming the stroke: "
                    + "\(alerts.map { "\($0.messageText) | \($0.informativeText)" })")
        try require(steps.isEmpty && document.history.entries.count == entries
                    && document.doc === before && document.doc?.layerCount == layers,
                    "\(tag) \(what): nothing recorded or changed: \(steps.map(\.tool)) "
                    + "\(document.history.entries.map(\.name))")
    }

    // Convert to Shape Layer of the holding work path, the shape options stroked.
    try require(opsCurrent(document)?.ops == [.unite, .subtract],
                "\(tag) the work path holds [unite, subtract]")
    var stroked = options
    stroked.setStroke(red)
    stroked.strokeWidth = 3
    ToolOptionsStore.shared.shape = stroked
    try requireStrokeRefused("Convert to Shape Layer") { controller.convertPathToShape(nil) }

    // The re-edit session of a holding path shape with no stroke, restyled with one in the bar.
    let made = try agentOK("add_shape_layer", document,
                           ["kind": "path", "subpaths": held, "fill": "#000000"])
    guard let index = made["layer"] as? Int, document.doc?.shapePayload(index)?.path?.ops != nil
    else { throw ProbeFailure(description: "\(tag) no holding path shape: \(made)") }
    document.undoManager?.removeAllActions()
    controller.editShapeLayer(index)
    guard controller.shapeEditSession != nil else {
        throw ProbeFailure(description: "\(tag) the path shape did not reopen: \(uiAlerts())")
    }
    ToolOptionsStore.shared.shape.setStroke(red)
    ToolOptionsStore.shared.shape.strokeWidth = 3
    controller.shapeStyleEdited()
    var committed = true
    try requireStrokeRefused("the re-edit given a stroke") {
        committed = controller.commitShapeEditSession()
    }
    try require(!committed && controller.shapeEditSession != nil,
                "\(tag) the refused re-edit keeps its session open")
}
