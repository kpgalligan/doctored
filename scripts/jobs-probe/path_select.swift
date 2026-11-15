import AppKit

// Path Selection and Direct Selection (`designs/v1-paths.md`, PathSelectSession.swift,
// ImageCanvasView+PathSelect.swift, EditorViewController+PathSelect.swift), run off screen:
//
// "path selection tools": the two tools' facts (names, symbols, the Pen's slot, A cycling the two
//   while P stays the Pen's), every mode's tool gate, and the three new menu items validating in
//   every document kind (`colourModeRows`).
// "path selection gestures": mouse and key events handed to the canvas view on a list path and
//   on a layer's vector mask, at 8 bits and once at 16: select, ⇧-select, move, ⌥-duplicate,
//   marquee, ⌫, Escape, an ⌥-click that places no copy; Direct Selection's anchor, marquee and
//   handle drags and ⌫. Each gesture is ONE undo step recording one `set_path` (or
//   `set_vector_mask`) that replays to the same shape;
//   the selection survives its own commit, a Fill Path and a non-path edit, and is dropped by an
//   undo, an agent's `set_path` and leaving the two tools; ⌫ owns Clear only while a selection is
//   held, and ⌥⌫ stays on.
// "path subpath booleans": Combine Selected Subpaths' validation and its one recorded
//   `combine_paths` step (`subpath_indices`), replayed; on a vector mask one `set_vector_mask`.
// "path fill and stroke sheets": Fill Path… and Stroke Path… driven through their own controls.

/// Three rectangles, the first two sharing the edge x = 60 (`subpath_indices` [1] of unite
/// merges them), the third apart.
private let selectRects: [[String: Any]] = [
    ["closed": true, "anchors": [[20, 20], [60, 20], [60, 70], [20, 70]]],
    ["closed": true, "anchors": [[60, 20], [100, 20], [100, 70], [60, 70]]],
    ["closed": true, "anchors": [[130, 90], [180, 90], [180, 140], [130, 140]]],
]

/// One open subpath whose middle anchor is smooth, for Direct Selection.
private let selectCurve: [[String: Any]] = [
    ["closed": false, "anchors": [
        [30, 120],
        ["x": 80, "y": 100, "in": [60, 100], "out": [100, 100], "smooth": true],
        [140, 130],
    ]],
]

// MARK: - Events

private func pathSelectEvent(
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
private func pathSelectDrag(
    _ canvas: ImageCanvasView, from: CGPoint, to: CGPoint, _ flags: NSEvent.ModifierFlags = []
) throws {
    canvas.mouseDown(with: try pathSelectEvent(canvas, .leftMouseDown, from, flags))
    if from != to {
        let middle = CGPoint(x: (from.x + to.x) / 2, y: (from.y + to.y) / 2)
        canvas.mouseDragged(with: try pathSelectEvent(canvas, .leftMouseDragged, middle, flags))
        canvas.mouseDragged(with: try pathSelectEvent(canvas, .leftMouseDragged, to, flags))
    }
    canvas.mouseUp(with: try pathSelectEvent(canvas, .leftMouseUp, to, flags))
}

private func pathSelectClick(
    _ canvas: ImageCanvasView, _ point: CGPoint, _ flags: NSEvent.ModifierFlags = []
) throws {
    try pathSelectDrag(canvas, from: point, to: point, flags)
}

private func pathSelectKey(_ canvas: ImageCanvasView, _ characters: String, _ code: UInt16) {
    guard let event = NSEvent.keyEvent(
        with: .keyDown, location: .zero, modifierFlags: [],
        timestamp: ProcessInfo.processInfo.systemUptime, windowNumber: 0, context: nil,
        characters: characters, charactersIgnoringModifiers: characters, isARepeat: false,
        keyCode: code) else { return }
    canvas.keyDown(with: event)
}

// MARK: - Shapes

private func shapeOf(_ subpaths: [[String: Any]], _ rule: String = "nonzero") throws -> PathShape {
    try PathShape(subpathsJSON: subpaths, fillRuleJSON: rule)
}

/// `shape` with subpath `subpath` (all anchors and handles) moved by (`dx`, `dy`).
private func moved(_ shape: PathShape, _ subpaths: [Int], _ dx: CGFloat, _ dy: CGFloat)
    -> PathShape {
    var out = shape
    for index in subpaths {
        out.subpaths[index].anchors = shape.subpaths[index].anchors.map { $0.moved(dx: dx, dy: dy) }
    }
    return out
}

/// The current path as the document holds it.
private func currentShape(_ document: ImageDocument) -> PathShape? {
    document.resolvedPath(document.currentPath)
}

/// Opens a fixture with `subpaths` as its work path, the current path, and `tool` selected.
private func pathSelectFixture(
    _ subpaths: [[String: Any]], tool: EditorTool, sixteen: Bool = false,
    width: Int = 200, height: Int = 160
) throws -> (ImageDocument, EditorViewController) {
    let document = try sixteen ? precisionFixture(width: width, height: height)
        : fixture(width: width, height: height)
    do {
        let controller = try editor(document)
        _ = try agentOK("set_path", document, ["subpaths": subpaths])
        guard let work = document.doc?.paths.workPath else {
            throw ProbeFailure(description: "no work path")
        }
        controller.setCurrentPath(.path(work.id))
        controller.selectTool(tool)
        try require(controller.canvas.tool == tool, "\(tool) is the tool")
        document.undoManager?.removeAllActions()
        return (document, controller)
    } catch {
        document.close()
        throw error
    }
}

/// One gesture's bookkeeping: ONE history entry named `name`, ONE recorded `tool` step whose
/// replay (after an undo) gives the same shape, and the undo itself restoring `before` exactly.
private func requireOneStep(
    _ document: ImageDocument, _ steps: [ActionStep], entries: Int, name: String,
    tool: String = "set_path", before: PathShape, _ tag: String
) throws {
    guard let after = currentShape(document) else {
        throw ProbeFailure(description: "\(tag) the current path is gone")
    }
    try require(document.history.entries.count == entries + 1
                && document.undoManager?.undoActionName == name,
                "\(tag) one \"\(name)\" step: \(document.history.entries.map(\.name))")
    try require(steps.count == 1 && steps[0].tool == tool,
                "\(tag) one recorded \(tool): \(steps.map(\.tool))")
    document.undoManager?.undo()
    try require(currentShape(document) == before, "\(tag) one undo restores the path exactly")
    let replay = ActionPlayer.runSingle(steps[0], on: document, undoName: "Replay")
    try require(replay.ok && currentShape(document) == after,
                "\(tag) the recorded step replays to the same path: \(replay.failures)")
    document.undoManager?.undo()
    document.undoManager?.redo()
}

// MARK: - "path selection tools"

func verifyPathSelectionTools() throws {
    let tag = "[path selection tools]"
    var failures: [String] = []
    for tool in [EditorTool.pathSelect, .directSelect] {
        if tool.keyCharacter != "a" || tool.cursor != .arrow || tool.planned
            || NSImage(systemSymbolName: tool.symbol, accessibilityDescription: nil) == nil {
            failures.append("\(tool.displayName): key \(tool.keyCharacter), symbol \(tool.symbol)")
        }
    }
    try require(EditorTool.pathSelect.displayName == "Path Selection"
                && EditorTool.directSelect.displayName == "Direct Selection",
                "\(tag) the names")
    let slot = EditorTool.railGroups[EditorTool.pen.railGroupIndex]
    if slot != [.pen, .pathSelect, .directSelect] { failures.append("the Pen's slot is \(slot)") }
    let glyphs = slot.map(\.fallbackGlyph)
    if Set(glyphs).count != glyphs.count { failures.append("the slot's glyphs \(glyphs)") }
    let cycle = [EditorTool.pen, .pathSelect, .directSelect].map {
        EditorTool.tool(forKey: "a", current: $0, last: nil)
    }
    if cycle != [.pathSelect, .directSelect, .pathSelect] { failures.append("A cycles \(cycle)") }
    if EditorTool.tool(forKey: "p", current: .pen, last: nil) != .pen
        || EditorTool.tool(forKey: "p", current: .pathSelect, last: nil) != .pen {
        failures.append("P is not the Pen's alone")
    }
    for tool in [EditorTool.pathSelect, .directSelect] {
        let gates = [
            ("Gray", GrayDocument.allows(tool)), ("Lab", LabDocument.allows(tool)),
            ("Indexed", IndexedDocument.allows(tool)), ("Bitmap", BitmapDocument.allows(tool)),
            ("CMYK 8", CmykDocument.allows(tool, depth: 8)),
            ("CMYK 16", CmykDocument.allows(tool, depth: 16)),
        ]
        for (mode, allowed) in gates where !allowed {
            failures.append("\(tool.displayName) refused by the \(mode) gate")
        }
    }
    // Every document kind: the tools are offered, and with a subpath selected the three new
    // items validate (each mode's closed `selectors` set, the float list, `sixteenBitActions`).
    for row in colourModeRows() {
        let (document, controller, _) = try openColourModeRow(row, tag: tag)
        defer { document.close() }
        for tool in [EditorTool.pathSelect, .directSelect]
        where !DocumentPrecision.allows(tool, document: document)
            || !uiValidates(controller, NSStringFromSelector(tool.action)) {
            failures.append("\(row.name): \(tool.displayName) is not offered")
        }
        _ = try agentOK("set_path", document, ["subpaths": selectRects])
        guard let work = document.doc?.paths.workPath else {
            failures.append("\(row.name): no work path")
            continue
        }
        controller.setCurrentPath(.path(work.id))
        controller.selectTool(.pathSelect)
        try pathSelectClick(controller.canvas, CGPoint(x: 40, y: 45))
        for selector in [
            "fillCurrentPathWithOptions:", "strokeCurrentPathWithOptions:", "combineSelectedSubpaths:",
        ] where !uiValidates(controller, selector) {
            failures.append("\(row.name): \(selector) is off with a subpath selected")
        }
    }
    try require(failures.isEmpty, "\(tag) \(failures.joined(separator: "; "))")
    print("PASS: the two tools' facts, A cycling them, every mode's gate, and the three items "
          + "in every document kind")
}

// MARK: - "path selection gestures"

func verifyPathSelectionGestures() throws {
    var failures: [String] = []
    for part in [
        { try verifyPathSelectionOnList(sixteen: false) },
        { try verifyPathSelectionOnList(sixteen: true) },
        verifyPathSelectionKeepsAndDrops, verifyDirectSelection, verifyPathSelectionOnVectorMask,
    ] {
        do { try part() } catch { failures.append(String(describing: error)) }
    }
    try require(failures.isEmpty, failures.joined(separator: " ;; "))
    print("PASS: Path Selection and Direct Selection gestures, each one recorded step, at 8 and "
          + "16 bits and on a vector mask")
}

private func verifyPathSelectionOnList(sixteen: Bool) throws {
    let tag = "[path selection \(sixteen ? 16 : 8)]"
    let (document, controller) = try pathSelectFixture(
        selectRects, tool: .pathSelect, sixteen: sixteen)
    defer {
        if ActionRecorder.shared.isRecording { _ = ActionRecorder.shared.stop() }
        document.close()
    }
    let canvas: ImageCanvasView = controller.canvas
    guard let base = currentShape(document) else { throw ProbeFailure(description: "\(tag) no path") }
    // A press inside a fill takes that subpath; ⇧ adds another.
    var entries = document.history.entries.count
    try pathSelectClick(canvas, CGPoint(x: 40, y: 45))
    try require(canvas.pathSelectSession.selection.subpaths == [0], "\(tag) the press selects 0")
    try pathSelectClick(canvas, CGPoint(x: 80, y: 45), .shift)
    try require(canvas.pathSelectSession.selection.subpaths == [0, 1], "\(tag) ⇧ adds 1")
    try require(document.history.entries.count == entries, "\(tag) selecting is no edit")

    // A drag moves exactly the selected subpaths, as one step; the selection survives it.
    var steps = try uiRecorded {
        try pathSelectDrag(canvas, from: CGPoint(x: 40, y: 45), to: CGPoint(x: 52, y: 51))
    }
    try require(currentShape(document) == moved(base, [0, 1], 12, 6),
                "\(tag) 0 and 1 moved by (12, 6), 2 kept: \(String(describing: currentShape(document)))")
    try require(steps.first?.note == "Path Selection tool", "\(tag) the step's note: \(steps.map(\.note))")
    try require(canvas.pathSelectSession.selection.subpaths == [0, 1],
                "\(tag) the selection survives its own commit")
    try requireOneStep(document, steps, entries: entries, name: "Move Subpaths", before: base, tag)
    try require(canvas.pathSelectSession.selection.isEmpty, "\(tag) an undo drops the selection")
    guard let movedShape = currentShape(document) else { throw ProbeFailure(description: "\(tag) gone") }

    // ⌥-drag: copies of the selected subpath, appended and moved; the original stays.
    try pathSelectClick(canvas, CGPoint(x: 140, y: 100))
    entries = document.history.entries.count
    steps = try uiRecorded {
        try pathSelectDrag(canvas, from: CGPoint(x: 140, y: 100), to: CGPoint(x: 110, y: 100),
                           .option)
    }
    var copied = movedShape
    copied.subpaths.append(moved(movedShape, [2], -30, 0).subpaths[2])
    try require(currentShape(document) == copied, "\(tag) ⌥ appends a moved copy of 2")
    try require(canvas.pathSelectSession.selection.subpaths == [3], "\(tag) the copy is selected")
    try requireOneStep(
        document, steps, entries: entries, name: "Duplicate Subpaths", before: movedShape, tag)

    // A marquee over the third subpath and its copy selects those two; ⌫ removes them, with
    // Clear standing down and ⌥⌫ still on; the pixels are untouched.
    _ = try agentOK("select_rect", document, ["x": 0, "y": 0, "width": 20, "height": 20])
    document.undoManager?.removeAllActions()
    let pixelsBefore = document.doc?.layerPixels(0)
    try require(uiValidates(controller, "clearSelection:"), "\(tag) Clear is on with no path selection")
    try pathSelectDrag(canvas, from: CGPoint(x: 190, y: 155), to: CGPoint(x: 95, y: 82))
    try require(canvas.pathSelectSession.selection.subpaths == [2, 3],
                "\(tag) the marquee selects 2 and 3: \(canvas.pathSelectSession.selection) "
                + "at magnification \(canvas.magnification)")
    try require(!uiValidates(controller, "clearSelection:"), "\(tag) Clear stands down for ⌫")
    try require(uiValidates(controller, "fillWithForeground:"), "\(tag) ⌥⌫ stays on")
    guard let marqueed = currentShape(document) else { throw ProbeFailure(description: "\(tag) gone") }
    entries = document.history.entries.count
    steps = uiRecorded { pathSelectKey(canvas, "\u{7f}", 51) }
    var deleted = marqueed
    deleted.subpaths.removeLast(2)
    try require(currentShape(document) == deleted, "\(tag) ⌫ removes subpaths 2 and 3")
    try require(document.doc?.layerPixels(0) == pixelsBefore, "\(tag) ⌫ leaves the pixels")
    try requireOneStep(
        document, steps, entries: entries, name: "Delete Subpaths", before: marqueed, tag)

    // Escape drops the selection; a click on nothing drops it too.
    try pathSelectClick(canvas, CGPoint(x: 52, y: 51))
    try require(canvas.pathSelectSession.selection.subpaths == [0], "\(tag) 0 selected")
    pathSelectKey(canvas, "\u{1b}", 53)
    try require(canvas.pathSelectSession.selection.isEmpty, "\(tag) Escape drops the selection")
    try pathSelectClick(canvas, CGPoint(x: 52, y: 51))
    try pathSelectClick(canvas, CGPoint(x: 190, y: 10))
    try require(canvas.pathSelectSession.selection.isEmpty, "\(tag) a click on nothing drops it")

    // An ⌥-click that never moves places no copy: it selects the subpath it pressed, as a
    // plain click does, and adds no step.
    entries = document.history.entries.count
    try pathSelectClick(canvas, CGPoint(x: 92, y: 51), .option)
    try require(canvas.pathSelectSession.selection.subpaths == [1]
                && document.history.entries.count == entries,
                "\(tag) an ⌥-click selects 1 and adds no step: "
                + "\(canvas.pathSelectSession.selection), \(document.history.entries.map(\.name))")

    // With no current path a press changes nothing and adds no step.
    controller.setCurrentPath(nil)
    entries = document.history.entries.count
    let still = document.doc
    try pathSelectDrag(canvas, from: CGPoint(x: 52, y: 51), to: CGPoint(x: 70, y: 70))
    try require(document.doc === still && document.history.entries.count == entries,
                "\(tag) no current path: nothing changes")
}

/// The selection survives a Fill Path (which fills the selected subpath alone) and a non-path
/// edit; a path change by an agent drops it; leaving the tools drops it, so Fill Path under
/// another tool fills the whole path.
private func verifyPathSelectionKeepsAndDrops() throws {
    let tag = "[path selection kept and dropped]"
    let (document, controller) = try pathSelectFixture(selectRects, tool: .pathSelect)
    defer { document.close() }
    let canvas: ImageCanvasView = controller.canvas
    let width = document.doc?.width ?? 0
    func red(_ x: Int, _ y: Int) -> [UInt8] {
        guard let pixels = document.doc?.layerPixels(0) else { return [] }
        let at = (y * width + x) * 4
        return Array(pixels[at..<(at + 4)])
    }
    controller.paintColor = NSColor(srgbRed: 1, green: 0, blue: 0, alpha: 1)
    try pathSelectClick(canvas, CGPoint(x: 40, y: 45))
    let untouched = red(80, 45)
    controller.fillCurrentPath(nil)
    try require(red(40, 45) == [255, 0, 0, 255] && red(80, 45) == untouched,
                "\(tag) Fill Path fills subpath 0 alone: \(red(40, 45)) \(red(80, 45))")
    try require(canvas.pathSelectSession.selection.subpaths == [0],
                "\(tag) the selection survives a Fill Path")
    _ = try agentOK("set_layer_properties", document, ["layer": 0, "name": "Renamed"])
    try require(canvas.pathSelectSession.selection.subpaths == [0],
                "\(tag) the selection survives a non-path edit")
    _ = try agentOK("set_path", document, ["subpaths": Array(selectRects.prefix(2))])
    try require(canvas.pathSelectSession.selection.isEmpty, "\(tag) an agent's set_path drops it")
    try pathSelectClick(canvas, CGPoint(x: 40, y: 45))
    try require(canvas.pathSelectSession.selection.subpaths == [0], "\(tag) 0 selected again")
    controller.selectTool(.brush)
    try require(canvas.pathSelectSession.selection.isEmpty, "\(tag) leaving the tools drops it")
    controller.fillCurrentPath(nil)
    try require(red(80, 45) == [255, 0, 0, 255],
                "\(tag) Fill Path under another tool fills the whole path: \(red(80, 45))")
}

private func verifyDirectSelection() throws {
    let tag = "[direct selection]"
    let (document, controller) = try pathSelectFixture(selectCurve, tool: .directSelect)
    defer {
        if ActionRecorder.shared.isRecording { _ = ActionRecorder.shared.stop() }
        document.close()
    }
    let canvas: ImageCanvasView = controller.canvas
    guard let base = currentShape(document) else { throw ProbeFailure(description: "\(tag) no path") }
    // A press on the smooth anchor selects it; a drag moves it with its handles.
    var entries = document.history.entries.count
    var steps = try uiRecorded {
        try pathSelectDrag(canvas, from: CGPoint(x: 80, y: 100), to: CGPoint(x: 90, y: 92))
    }
    var expected = base
    expected.subpaths[0].anchors[1] = base.subpaths[0].anchors[1].moved(dx: 10, dy: -8)
    try require(currentShape(document) == expected, "\(tag) the anchor and its handles moved")
    try require(canvas.pathSelectSession.selection.anchors
                == [PathAnchorRef(subpath: 0, index: 1)], "\(tag) the anchor is selected")
    try requireOneStep(
        document, steps, entries: entries, name: "Move Anchor Points", before: base, tag)
    guard let movedAnchor = currentShape(document) else { throw ProbeFailure(description: "gone") }

    // The selected smooth anchor's out handle follows the pointer; the in handle stays in line.
    try pathSelectClick(canvas, CGPoint(x: 90, y: 92))
    entries = document.history.entries.count
    steps = try uiRecorded {
        try pathSelectDrag(canvas, from: CGPoint(x: 110, y: 92), to: CGPoint(x: 90, y: 72))
    }
    guard let handled = currentShape(document)?.subpaths[0].anchors[1] else {
        throw ProbeFailure(description: "\(tag) gone")
    }
    let inReach = hypot(handled.handleIn.x - 90, handled.handleIn.y - 92)
    try require(handled.handleOut == CGPoint(x: 90, y: 72) && abs(inReach - 20) < 1e-3
                && abs(handled.handleIn.x - 90) < 1e-3 && abs(handled.handleIn.y - 112) < 1e-3,
                "\(tag) the out handle follows and the in handle stays opposite: \(handled)")
    try requireOneStep(
        document, steps, entries: entries, name: "Move Handle", before: movedAnchor, tag)

    // A marquee selects the two end anchors; a drag moves them together and nothing else.
    guard let handledShape = currentShape(document) else { throw ProbeFailure(description: "gone") }
    try pathSelectDrag(canvas, from: CGPoint(x: 10, y: 110), to: CGPoint(x: 195, y: 155))
    try require(canvas.pathSelectSession.selection.anchors
                == [PathAnchorRef(subpath: 0, index: 0), PathAnchorRef(subpath: 0, index: 2)],
                "\(tag) the marquee selects the two ends")
    entries = document.history.entries.count
    steps = try uiRecorded {
        try pathSelectDrag(canvas, from: CGPoint(x: 30, y: 120), to: CGPoint(x: 30, y: 135))
    }
    var ends = handledShape
    for index in [0, 2] {
        ends.subpaths[0].anchors[index] = handledShape.subpaths[0].anchors[index].moved(dx: 0, dy: 15)
    }
    try require(currentShape(document) == ends, "\(tag) the two ends moved together")
    try requireOneStep(
        document, steps, entries: entries, name: "Move Anchor Points", before: handledShape, tag)

    // ⌫ removes the selected anchors; an agent's set_path drops the selection.
    try pathSelectClick(canvas, CGPoint(x: 30, y: 135))
    guard let beforeDelete = currentShape(document) else { throw ProbeFailure(description: "gone") }
    entries = document.history.entries.count
    steps = uiRecorded { pathSelectKey(canvas, "\u{7f}", 51) }
    var deleted = beforeDelete
    deleted.subpaths[0].anchors.remove(at: 0)
    try require(currentShape(document) == deleted, "\(tag) ⌫ removes the anchor")
    try requireOneStep(
        document, steps, entries: entries, name: "Delete Anchor Points", before: beforeDelete, tag)
    try pathSelectClick(canvas, CGPoint(x: 140, y: 145))
    try require(!canvas.pathSelectSession.selection.isEmpty, "\(tag) an anchor is selected")
    _ = try agentOK("set_path", document, ["subpaths": selectRects])
    try require(canvas.pathSelectSession.selection.isEmpty, "\(tag) an agent's set_path drops it")
}

private func verifyPathSelectionOnVectorMask() throws {
    let tag = "[path selection vector mask]"
    let (document, controller) = try pathSelectFixture(selectRects, tool: .pathSelect)
    defer {
        if ActionRecorder.shared.isRecording { _ = ActionRecorder.shared.stop() }
        document.close()
    }
    _ = try agentOK("set_vector_mask", document, ["layer": 0, "subpaths": selectRects])
    guard let layerID = document.doc?.layerInfo(0)?.id else {
        throw ProbeFailure(description: "\(tag) no layer")
    }
    controller.setCurrentPath(.vectorMask(layerID: layerID))
    document.undoManager?.removeAllActions()
    let canvas: ImageCanvasView = controller.canvas
    guard let base = currentShape(document) else { throw ProbeFailure(description: "\(tag) no mask") }
    try pathSelectClick(canvas, CGPoint(x: 80, y: 45))
    let entries = document.history.entries.count
    let steps = try uiRecorded {
        try pathSelectDrag(canvas, from: CGPoint(x: 80, y: 45), to: CGPoint(x: 80, y: 60))
    }
    try require(currentShape(document) == moved(base, [1], 0, 15), "\(tag) subpath 1 moved down")
    try requireOneStep(document, steps, entries: entries, name: "Move Subpaths",
                       tool: "set_vector_mask", before: base, tag)
}

// MARK: - "path subpath booleans"

func verifyPathSubpathBooleans() throws {
    let tag = "[path subpath booleans]"
    let (document, controller) = try pathSelectFixture(selectRects, tool: .pathSelect)
    defer {
        if ActionRecorder.shared.isRecording { _ = ActionRecorder.shared.stop() }
        document.close()
    }
    let canvas: ImageCanvasView = controller.canvas
    let selector = "combineSelectedSubpaths:"
    try require(!uiValidates(controller, selector), "\(tag) off with nothing selected")
    try pathSelectClick(canvas, CGPoint(x: 80, y: 45))
    try require(uiValidates(controller, selector, tag: 0), "\(tag) on with subpath 1 selected")
    guard let base = currentShape(document) else { throw ProbeFailure(description: "\(tag) no path") }
    let entries = document.history.entries.count
    let steps = uiRecorded { controller.commitCombineSelectedSubpaths(.unite) }
    guard let united = currentShape(document) else { throw ProbeFailure(description: "\(tag) gone") }
    try require(united.subpaths.count == 2 && united.anchorCount == 8,
                "\(tag) 0 and 1 merged, 2 kept: \(united.subpaths.map(\.anchors.count))")
    try require(steps.first?.arguments["subpath_indices"] as? [Int] == [1],
                "\(tag) the step carries subpath_indices [1]: \(steps.map(\.arguments))")
    try require(canvas.pathSelectSession.selection.isEmpty,
                "\(tag) the selection does not outlive the combine")
    try requireOneStep(document, steps, entries: entries, name: "Combine Subpaths",
                       tool: "combine_paths", before: base, tag)

    // On a vector mask the combine is one set_vector_mask step.
    _ = try agentOK("set_vector_mask", document, ["layer": 0, "subpaths": selectRects])
    guard let layerID = document.doc?.layerInfo(0)?.id else { throw ProbeFailure(description: tag) }
    controller.setCurrentPath(.vectorMask(layerID: layerID))
    document.undoManager?.removeAllActions()
    guard let mask = currentShape(document) else { throw ProbeFailure(description: "\(tag) no mask") }
    try pathSelectClick(canvas, CGPoint(x: 80, y: 45))
    let maskEntries = document.history.entries.count
    let maskSteps = uiRecorded { controller.commitCombineSelectedSubpaths(.unite) }
    try require(currentShape(document)?.subpaths.count == 2, "\(tag) the mask is combined")
    try requireOneStep(document, maskSteps, entries: maskEntries, name: "Combine Subpaths",
                       tool: "set_vector_mask", before: mask, tag)
    print("PASS: Combine Selected Subpaths validates with a selection, records one combine_paths "
          + "with subpath_indices on a list path and one set_vector_mask on a vector mask")
}

// MARK: - "path fill and stroke sheets"

/// The sheet `open` presents, once it is on screen. Also "paths panel stroke float"'s
/// (`ui_float_sheets.swift`).
func pathSheet<T: NSViewController>(
    _ controller: EditorViewController, _ type: T.Type, _ open: () -> Void
) throws -> T {
    let known = (controller.presentedViewControllers ?? []).map(ObjectIdentifier.init)
    open()
    let opened = {
        (controller.presentedViewControllers ?? []).compactMap { $0 as? T }
            .last { !known.contains(ObjectIdentifier($0)) }
    }
    try pumpUntil({ opened()?.view.window != nil }, seconds: 30)
    guard let sheet = opened() else { throw ProbeFailure(description: "no \(T.self)") }
    return sheet
}

func verifyPathFillStrokeSheets() throws {
    let tag = "[path fill and stroke sheets]"
    let rects = Array(selectRects.prefix(2))
    let (document, controller) = try pathSelectFixture(rects, tool: .pathSelect)
    let twin = try fixture(width: 200, height: 160)
    defer {
        if ActionRecorder.shared.isRecording { _ = ActionRecorder.shared.stop() }
        document.close()
        twin.close()
    }
    _ = try agentOK("set_path", twin, ["subpaths": rects])
    let store = ToolOptionsStore.shared
    let savedFill = store.fillDialog, savedEraser = store.eraser
    defer {
        store.fillDialog = savedFill
        store.eraser = savedEraser
    }
    controller.backgroundColor = NSColor(srgbRed: 0.2, green: 0.4, blue: 0.8, alpha: 1)

    // Fill Path…: Background, Multiply, 50 %; no Pattern row.
    let fill = try pathSheet(controller, FillSheetController.self) {
        controller.fillCurrentPathWithOptions(nil)
    }
    let contents = try uiControl(fill.view, id: "fill-contents", as: NSPopUpButton.self)
    try require(!contents.itemTitles.contains("Pattern"), "\(tag) no Pattern: \(contents.itemTitles)")
    contents.selectItem(withTitle: "Background Color")
    let mode = try uiControl(fill.view, id: "fill-mode", as: NSPopUpButton.self)
    mode.selectItem(withTag: Int(DR_BLEND_MULTIPLY.rawValue))
    try uiControl(fill.view, id: "fill-opacity", as: NSTextField.self).doubleValue = 50
    let steps = try uiRecorded {
        try uiSend(try uiControl(fill.view, id: "fill-apply", as: NSButton.self), "\(tag) Apply")
    }
    let arguments = steps.first?.arguments ?? [:]
    try require(steps.map(\.tool) == ["fill_path"] && arguments["path"] as? String == "Work Path"
                && arguments["blend_mode"] as? String == "Multiply"
                && (arguments["opacity"] as? NSNumber)?.doubleValue == 0.5
                && (arguments["color"] as? String)?.uppercased().hasPrefix("#3366CC") == true
                && arguments["color_space"] == nil,
                "\(tag) one fill_path with the sheet's choices: \(steps.map(\.arguments))")
    _ = try agentOK("fill_path", twin, [
        "path": "Work Path", "color": "#3366CC", "blend_mode": "Multiply", "opacity": 0.5,
        "layer": 0,
    ])
    try require(document.doc?.layerPixels(0) == twin.doc?.layerPixels(0),
                "\(tag) the pixels equal fill_path called over MCP")

    // Stroke Path… with the Eraser lowers alpha along the outline; no colour key.
    var eraser = store.eraser
    eraser.size = 6
    eraser.hardness = 100
    eraser.opacity = 100
    eraser.flow = 100
    store.eraser = eraser
    let width = document.doc?.width ?? 0
    func alpha(_ x: Int, _ y: Int) -> UInt8 { document.doc?.layerPixels(0)?[(y * width + x) * 4 + 3] ?? 0 }
    try require(alpha(20, 45) == 255, "\(tag) opaque before the stroke")
    let stroke = try pathSheet(controller, PathStrokeSheetController.self) {
        controller.strokeCurrentPathWithOptions(nil)
    }
    try uiControl(stroke.view, id: "stroke-path-tool", as: NSPopUpButton.self).selectItem(at: 1)
    let strokeSteps = try uiRecorded {
        try uiSend(try uiControl(stroke.view, id: "stroke-path-apply", as: NSButton.self),
                   "\(tag) Apply")
    }
    let strokeArguments = strokeSteps.first?.arguments ?? [:]
    try require(strokeSteps.map(\.tool) == ["stroke_path"]
                && strokeArguments["tool"] as? String == "eraser"
                && strokeArguments["color"] == nil && strokeArguments["blend_mode"] == nil,
                "\(tag) one stroke_path with the Eraser: \(strokeSteps.map(\.arguments))")
    try require(alpha(20, 45) < 255, "\(tag) the Eraser lowered alpha on the outline")

    // With subpath 1 selected the step carries that subpath alone and subpath 0 is untouched.
    try pathSelectClick(controller.canvas, CGPoint(x: 80, y: 45))
    let before = document.doc?.layerPixels(0) ?? []
    let one = try pathSheet(controller, FillSheetController.self) {
        controller.fillCurrentPathWithOptions(nil)
    }
    try uiControl(one.view, id: "fill-contents", as: NSPopUpButton.self)
        .selectItem(withTitle: "Black")
    try uiControl(one.view, id: "fill-mode", as: NSPopUpButton.self)
        .selectItem(withTag: Int(DR_BLEND_NORMAL.rawValue))
    try uiControl(one.view, id: "fill-opacity", as: NSTextField.self).doubleValue = 100
    let oneSteps = try uiRecorded {
        try uiSend(try uiControl(one.view, id: "fill-apply", as: NSButton.self), "\(tag) Apply")
    }
    let oneArguments = oneSteps.first?.arguments ?? [:]
    try require(oneArguments["path"] == nil
                && (oneArguments["subpaths"] as? [[String: Any]])?.count == 1,
                "\(tag) the step carries the one selected subpath: \(oneArguments.keys.sorted())")
    let after = document.doc?.layerPixels(0) ?? []
    let inside0 = (45 * width + 40) * 4, inside1 = (45 * width + 80) * 4
    try require(Array(after[inside0..<(inside0 + 4)]) == Array(before[inside0..<(inside0 + 4)])
                && after[inside1] == 0 && after[inside1 + 3] == 255,
                "\(tag) subpath 1 filled black, subpath 0 untouched")
    print("PASS: Fill Path… and Stroke Path… record fill_path and stroke_path with the sheets' "
          + "choices, and paint only the selected subpaths while some are selected")
}
