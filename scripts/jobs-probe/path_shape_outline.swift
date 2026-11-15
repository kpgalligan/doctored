import AppKit

// A path shape layer's outline as the current path (`designs/v1-paths.md`,
// ShapeOutlineTarget.swift, EditorViewController+ShapeOutline.swift), run off screen:
//
// "path shape outline target": the Paths panel's shape-path row appears for an active path shape
//   and for no other layer; a Direct Selection press with no current path adopts the active path
//   shape's outline (and still beeps on a rectangle shape, a raster layer, and a path shape that
//   was rasterized meanwhile); an anchor drag, a Path Selection subpath drag, Combine Selected
//   Subpaths and ⌫ are each ONE history entry recording one `edit_shape_layer` that replays to the
//   same outline, and the first leaves the layer's pixels equal to a twin's `add_shape_layer` of the
//   expected outline; the selection survives its own commit (under a 30° rotation too); deleting
//   every subpath beeps and adds no step; changing the active layer and rasterizing the layer drop
//   the target, after which the Pen makes a work path again; at 8 and 16 bits and in CMYK 8.

/// Two rectangles sharing the edge x = 60 and a third apart: the outline the cases edit.
private let outlineRects: [[String: Any]] = [
    ["closed": true, "anchors": [[20, 20], [60, 20], [60, 70], [20, 70]]],
    ["closed": true, "anchors": [[60, 20], [100, 20], [100, 70], [60, 70]]],
    ["closed": true, "anchors": [[130, 90], [180, 90], [180, 140], [130, 140]]],
]

private func outlineEvent(
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

/// A press at `from`, two drag ticks and a release at `to` (a click when they are equal).
private func outlineDrag(_ canvas: ImageCanvasView, from: CGPoint, to: CGPoint) throws {
    canvas.mouseDown(with: try outlineEvent(canvas, .leftMouseDown, from))
    if from != to {
        let middle = CGPoint(x: (from.x + to.x) / 2, y: (from.y + to.y) / 2)
        canvas.mouseDragged(with: try outlineEvent(canvas, .leftMouseDragged, middle))
        canvas.mouseDragged(with: try outlineEvent(canvas, .leftMouseDragged, to))
    }
    canvas.mouseUp(with: try outlineEvent(canvas, .leftMouseUp, to))
}

private func outlineDelete(_ canvas: ImageCanvasView) {
    guard let event = NSEvent.keyEvent(
        with: .keyDown, location: .zero, modifierFlags: [],
        timestamp: ProcessInfo.processInfo.systemUptime, windowNumber: 0, context: nil,
        characters: "\u{7f}", charactersIgnoringModifiers: "\u{7f}", isARepeat: false,
        keyCode: 51) else { return }
    canvas.keyDown(with: event)
}

private func outlineMoved(_ shape: PathShape, subpath: Int, anchor: Int? = nil,
                          _ dx: CGFloat, _ dy: CGFloat) -> PathShape {
    var out = shape
    for index in out.subpaths[subpath].anchors.indices where anchor == nil || anchor == index {
        out.subpaths[subpath].anchors[index] = shape.subpaths[subpath].anchors[index]
            .moved(dx: dx, dy: dy)
    }
    return out
}

/// The current path as the document holds it.
private func currentOutline(_ document: ImageDocument) -> PathShape? {
    document.resolvedPath(document.currentPath)
}

/// Adds a path shape of `subpaths` above layer 0 with a black fill and makes it active; its id.
private func addOutlineShape(
    _ document: ImageDocument, _ subpaths: [[String: Any]], transform: [Double]? = nil
) throws -> UInt64 {
    var arguments: [String: Any] = ["kind": "path", "subpaths": subpaths, "fill": "#000000"]
    if let transform { arguments["transform"] = transform }
    let made = try agentOK("add_shape_layer", document, arguments)
    guard let index = made["layer"] as? Int, let id = document.doc?.layerInfo(index)?.id else {
        throw ProbeFailure(description: "add_shape_layer: \(made)")
    }
    try require(document.activeLayerIndex == index, "the new shape layer is active")
    return id
}

/// One gesture's bookkeeping: ONE history entry named `name`, ONE recorded `edit_shape_layer`
/// whose replay (after an undo) gives the same outline, and the undo itself restoring `before`.
private func requireOneOutlineStep(
    _ document: ImageDocument, _ steps: [ActionStep], entries: Int, name: String,
    before: PathShape, _ tag: String
) throws {
    guard let after = currentOutline(document) else {
        throw ProbeFailure(description: "\(tag) the outline is gone")
    }
    try require(document.history.entries.count == entries + 1
                && document.undoManager?.undoActionName == name,
                "\(tag) one \"\(name)\" step: \(document.history.entries.map(\.name))")
    try require(steps.count == 1 && steps[0].tool == "edit_shape_layer"
                && steps[0].arguments["subpaths"] != nil && steps[0].arguments["x"] == nil,
                "\(tag) one recorded edit_shape_layer with subpaths: "
                + "\(steps.map { "\($0.tool) \($0.arguments.keys.sorted())" })")
    document.undoManager?.undo()
    try require(currentOutline(document) == before, "\(tag) one undo restores the outline exactly")
    let replay = ActionPlayer.runSingle(steps[0], on: document, undoName: "Replay")
    guard replay.ok, let replayed = currentOutline(document) else {
        throw ProbeFailure(description: "\(tag) the replay failed: \(replay.failures)")
    }
    try require(PathSelectSession.matches(replayed, after),
                "\(tag) the recorded step replays to the same outline")
    document.undoManager?.undo()
    document.undoManager?.redo()
}

// MARK: - "path shape outline target"

func verifyPathShapeOutlineTarget() throws {
    var failures: [String] = []
    for part in [
        verifyOutlineGestures, verifyOutlineDrops, verifyOutlineSixteen, verifyOutlineInCmyk,
        verifyOutlineTransformed,
    ] {
        do { try part() } catch { failures.append(String(describing: error)) }
    }
    try require(failures.isEmpty, failures.joined(separator: " ;; "))
    print("PASS: a path shape's outline is the selection tools' target: adopted by a press, each "
          + "gesture one recorded edit_shape_layer that replays, dropped when the layer stops being "
          + "the active path shape; at 8 and 16 bits, in CMYK 8 and under a rotation")
}

private func verifyOutlineGestures() throws {
    let tag = "[path shape outline 8]"
    let document = try fixture(width: 200, height: 160)
    defer {
        if ActionRecorder.shared.isRecording { _ = ActionRecorder.shared.stop() }
        document.close()
    }
    let controller = try editor(document)
    let canvas: ImageCanvasView = controller.canvas
    let layerID = try addOutlineShape(document, outlineRects)
    guard let doc = document.doc, let index = doc.layerIndex(id: layerID) else {
        throw ProbeFailure(description: "\(tag) no shape layer")
    }
    // The Paths panel's row: for the active path shape, and for no other layer.
    let rows = PathsPanelViewController.rows(of: doc, activeLayer: index)
    try require(rows.contains { $0.target == .shapeOutline(layerID: layerID) },
                "\(tag) the shape-path row is listed: \(rows.map(\.title))")
    try require(!PathsPanelViewController.rows(of: doc, activeLayer: 0).contains { $0.isShapeOutline },
                "\(tag) no shape-path row for the raster layer")
    controller.setCurrentPath(nil)
    controller.selectTool(.directSelect)
    document.undoManager?.removeAllActions()
    guard let base = doc.shapeOutlinePath(index) else {
        throw ProbeFailure(description: "\(tag) no outline")
    }

    // A press with no current path adopts the outline and takes the anchor; the drag moves it.
    var entries = document.history.entries.count
    var steps = try uiRecorded {
        try outlineDrag(canvas, from: CGPoint(x: 180, y: 140), to: CGPoint(x: 190, y: 150))
    }
    try require(document.currentPath == .shapeOutline(layerID: layerID),
                "\(tag) the press adopts the outline: \(String(describing: document.currentPath))")
    let dragged = outlineMoved(base, subpath: 2, anchor: 2, 10, 10)
    try require(currentOutline(document) == dragged,
                "\(tag) the anchor moved: \(String(describing: currentOutline(document)))")
    try require(canvas.pathSelectSession.selection.anchors == [PathAnchorRef(subpath: 2, index: 2)],
                "\(tag) the selection survives its own commit")
    try require(steps.first?.note == "Direct Selection tool", "\(tag) the note: \(steps.map(\.note))")
    // The pixels are a fresh layer's of the same outline.
    let twin = try fixture(width: 200, height: 160)
    defer { twin.close() }
    _ = try agentOK("add_shape_layer", twin, [
        "kind": "path", "subpaths": dragged.subpathsJSON, "fill": "#000000",
    ])
    let mine = document.doc?.layerInfo(index), theirs = twin.doc?.layerInfo(1)
    try require(document.doc?.layerPixels(index) == twin.doc?.layerPixels(1)
                && mine?.offsetX == theirs?.offsetX && mine?.offsetY == theirs?.offsetY
                && mine?.width == theirs?.width && mine?.height == theirs?.height,
                "\(tag) the layer equals a fresh add_shape_layer of the outline")
    try requireOneOutlineStep(
        document, steps, entries: entries, name: "Move Anchor Points", before: base, tag)

    // Path Selection: subpath 2 dragged as one step.
    controller.selectTool(.pathSelect)
    try outlineDrag(canvas, from: CGPoint(x: 155, y: 115), to: CGPoint(x: 155, y: 115))
    try require(canvas.pathSelectSession.selection.subpaths == [2], "\(tag) subpath 2 selected")
    entries = document.history.entries.count
    steps = try uiRecorded {
        try outlineDrag(canvas, from: CGPoint(x: 155, y: 115), to: CGPoint(x: 150, y: 105))
    }
    try require(currentOutline(document) == outlineMoved(dragged, subpath: 2, -5, -10),
                "\(tag) subpath 2 moved by (-5, -10)")
    try requireOneOutlineStep(
        document, steps, entries: entries, name: "Move Subpaths", before: dragged, tag)

    // Combine Selected Subpaths: subpath 1 united with the rest, one edit_shape_layer.
    guard let beforeCombine = currentOutline(document) else { throw ProbeFailure(description: tag) }
    try outlineDrag(canvas, from: CGPoint(x: 80, y: 45), to: CGPoint(x: 80, y: 45))
    entries = document.history.entries.count
    steps = uiRecorded { controller.commitCombineSelectedSubpaths(.unite) }
    try require(currentOutline(document)?.subpaths.count == 2,
                "\(tag) 0 and 1 merged: \(String(describing: currentOutline(document)))")
    try requireOneOutlineStep(
        document, steps, entries: entries, name: "Combine Subpaths", before: beforeCombine, tag)

    // ⌫ removes the selected subpath as one step; deleting the last one beeps and adds none.
    guard let beforeDelete = currentOutline(document) else { throw ProbeFailure(description: tag) }
    // The moved third rectangle, wherever the combine put it in the order.
    try outlineDrag(canvas, from: CGPoint(x: 150, y: 110), to: CGPoint(x: 150, y: 110))
    let picked = canvas.pathSelectSession.selection.subpaths
    guard picked.count == 1, let gone = picked.first else {
        throw ProbeFailure(description: "\(tag) one subpath selected: \(picked)")
    }
    entries = document.history.entries.count
    steps = uiRecorded { outlineDelete(canvas) }
    var remaining = beforeDelete
    remaining.subpaths.remove(at: gone)
    try require(currentOutline(document) == remaining, "\(tag) ⌫ removes subpath \(gone)")
    try requireOneOutlineStep(
        document, steps, entries: entries, name: "Delete Subpaths", before: beforeDelete, tag)
    try outlineDrag(canvas, from: CGPoint(x: 40, y: 45), to: CGPoint(x: 40, y: 45))
    try require(canvas.pathSelectSession.selection.subpaths == [0], "\(tag) the last one selected")
    entries = document.history.entries.count
    let still = document.doc
    outlineDelete(canvas)
    try require(document.doc === still && document.history.entries.count == entries
                && currentOutline(document) == remaining,
                "\(tag) deleting every subpath adds no step and keeps the outline")
}

/// The target is dropped when the layer stops being the active path shape; a rectangle shape and
/// a raster layer are never adopted.
private func verifyOutlineDrops() throws {
    let tag = "[path shape outline drops]"
    let document = try fixture(width: 200, height: 160)
    defer { document.close() }
    let controller = try editor(document)
    let canvas: ImageCanvasView = controller.canvas
    let layerID = try addOutlineShape(document, outlineRects)
    guard let index = document.doc?.layerIndex(id: layerID) else {
        throw ProbeFailure(description: "\(tag) no layer")
    }
    controller.setCurrentPath(nil)
    controller.selectTool(.directSelect)
    try outlineDrag(canvas, from: CGPoint(x: 20, y: 20), to: CGPoint(x: 20, y: 20))
    try require(document.currentPath == .shapeOutline(layerID: layerID), "\(tag) adopted")
    controller.setActiveLayer(0)
    try require(document.currentPath == nil, "\(tag) another active layer drops the target")
    var entries = document.history.entries.count
    try outlineDrag(canvas, from: CGPoint(x: 20, y: 20), to: CGPoint(x: 30, y: 30))
    try require(document.currentPath == nil && document.history.entries.count == entries,
                "\(tag) a press on a raster layer adopts nothing and adds no step")

    // Rasterized while it is the target and still active: the target goes, so a press beeps and
    // the Pen draws a work path again.
    controller.setActiveLayer(index)
    try outlineDrag(canvas, from: CGPoint(x: 20, y: 20), to: CGPoint(x: 20, y: 20))
    try require(document.currentPath == .shapeOutline(layerID: layerID), "\(tag) adopted again")
    _ = try agentOK("rasterize_artwork", document, ["layer": index])
    try require(document.currentPath == nil, "\(tag) a rasterized layer drops the target")
    entries = document.history.entries.count
    try outlineDrag(canvas, from: CGPoint(x: 20, y: 20), to: CGPoint(x: 30, y: 30))
    try require(document.currentPath == nil && document.history.entries.count == entries,
                "\(tag) a press on the rasterized layer adopts nothing and adds no step")
    controller.selectTool(.pen)
    try outlineDrag(canvas, from: CGPoint(x: 10, y: 150), to: CGPoint(x: 10, y: 150))
    guard let work = document.doc?.paths.workPath else {
        throw ProbeFailure(description: "\(tag) the Pen made no work path")
    }
    try require(document.currentPath == .path(work.id), "\(tag) the Pen's work path is current")

    // A rectangle shape is never adopted.
    controller.setCurrentPath(nil)
    _ = try agentOK("add_shape_layer", document, [
        "kind": "rect", "x": 120, "y": 20, "w": 40, "h": 30, "fill": "#336699",
    ])
    controller.selectTool(.directSelect)
    entries = document.history.entries.count
    try outlineDrag(canvas, from: CGPoint(x: 120, y: 20), to: CGPoint(x: 130, y: 30))
    try require(document.currentPath == nil && document.history.entries.count == entries,
                "\(tag) a rectangle shape is not adopted")
}

/// One anchor drag, one recorded step that replays, in a 16-bit document.
private func verifyOutlineSixteen() throws {
    let tag = "[path shape outline 16]"
    let document = try precisionFixture(width: 200, height: 160)
    defer {
        if ActionRecorder.shared.isRecording { _ = ActionRecorder.shared.stop() }
        document.close()
    }
    let controller = try editor(document)
    let layerID = try addOutlineShape(document, outlineRects)
    try requireAnchorDrag(document, controller, layerID: layerID, anchor: CGPoint(x: 180, y: 140),
                          subpath: 2, index: 2, tag)
}

/// The same in a CMYK 8 document: the layer keeps its inks through the re-render.
private func verifyOutlineInCmyk() throws {
    let tag = "[path shape outline cmyk]"
    guard let row = colourModeRows().first(where: { $0.name == "CMYK 8" }) else {
        throw ProbeFailure(description: "\(tag) no CMYK 8 row")
    }
    let (document, controller, _) = try openColourModeRow(row, tag: tag)
    defer {
        if ActionRecorder.shared.isRecording { _ = ActionRecorder.shared.stop() }
        document.close()
    }
    let layerID = try addOutlineShape(document, [
        ["closed": true, "anchors": [[8, 8], [40, 8], [40, 30], [8, 30]]],
    ])
    guard let index = document.doc?.layerIndex(id: layerID) else { throw ProbeFailure(description: tag) }
    let inks = (try? agentOK("get_shape_layer", document, ["layer": index]))?["shape"]
        .flatMap { ($0 as? [String: Any])?["fill_ink"] as? [Double] }
    try requireAnchorDrag(document, controller, layerID: layerID, anchor: CGPoint(x: 40, y: 30),
                          subpath: 0, index: 2, tag)
    let after = (try? agentOK("get_shape_layer", document, ["layer": index]))?["shape"]
        .flatMap { ($0 as? [String: Any])?["fill_ink"] as? [Double] }
    try require(inks != nil && inks == after, "\(tag) the fill ink is kept: \(String(describing: inks)) "
                + "→ \(String(describing: after))")
}

private func requireAnchorDrag(
    _ document: ImageDocument, _ controller: EditorViewController, layerID: UInt64,
    anchor: CGPoint, subpath: Int, index: Int, _ tag: String
) throws {
    let canvas: ImageCanvasView = controller.canvas
    guard let layer = document.doc?.layerIndex(id: layerID),
          let base = document.doc?.shapeOutlinePath(layer)
    else { throw ProbeFailure(description: "\(tag) no outline") }
    controller.setCurrentPath(nil)
    controller.selectTool(.directSelect)
    document.undoManager?.removeAllActions()
    let entries = document.history.entries.count
    let steps = try uiRecorded {
        try outlineDrag(canvas, from: anchor, to: CGPoint(x: anchor.x + 6, y: anchor.y + 4))
    }
    try require(document.currentPath == .shapeOutline(layerID: layerID), "\(tag) adopted")
    try require(currentOutline(document) == outlineMoved(base, subpath: subpath, anchor: index, 6, 4),
                "\(tag) the anchor moved: \(String(describing: currentOutline(document)))")
    try requireOneOutlineStep(
        document, steps, entries: entries, name: "Move Anchor Points", before: base, tag)
}

/// Under a 30° rotation the read-back differs from the session's shape by floating-point error
/// only, so the selection survives its own commit and the anchor lands where it was dragged.
private func verifyOutlineTransformed() throws {
    let tag = "[path shape outline rotated]"
    let document = try fixture(width: 200, height: 160)
    defer {
        if ActionRecorder.shared.isRecording { _ = ActionRecorder.shared.stop() }
        document.close()
    }
    let controller = try editor(document)
    let canvas: ImageCanvasView = controller.canvas
    let turn = Double.pi / 6
    let layerID = try addOutlineShape(
        document, [["closed": true, "anchors": [[60, 40], [120, 40], [120, 90], [60, 90]]]],
        transform: [cos(turn), -sin(turn), sin(turn), cos(turn)])
    // The panel row's way in: the outline made current explicitly.
    controller.setCurrentPath(.shapeOutline(layerID: layerID))
    controller.selectTool(.directSelect)
    document.undoManager?.removeAllActions()
    guard let base = currentOutline(document) else { throw ProbeFailure(description: "\(tag) none") }
    let corner = base.subpaths[0].anchors[1].point
    let entries = document.history.entries.count
    let steps = try uiRecorded {
        try outlineDrag(canvas, from: corner, to: CGPoint(x: corner.x + 6, y: corner.y + 4))
    }
    guard let moved = currentOutline(document)?.subpaths[0].anchors[1].point else {
        throw ProbeFailure(description: "\(tag) gone")
    }
    try require(abs(moved.x - corner.x - 6) < 1e-3 && abs(moved.y - corner.y - 4) < 1e-3,
                "\(tag) the corner moved by (6, 4): \(corner) → \(moved)")
    try require(canvas.pathSelectSession.selection.anchors == [PathAnchorRef(subpath: 0, index: 1)],
                "\(tag) the selection survives its own commit under the rotation")
    try requireOneOutlineStep(
        document, steps, entries: entries, name: "Move Anchor Points", before: base, tag)
}
