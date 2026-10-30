import AppKit

// The Move tool's ⌥ duplicate family — ⌥+arrow over a selection
// and with none, ⌥-drag with no selection, ⌥⌘T and the ⌥ cursor, each against
// its MCP twin — and the selected-pixels polish: the drag snapping to the
// float's opaque bounds and the delayed-reveal wait panel. Every case calls
// methods directly (`canvas.keyDown(with:)`, the Move callbacks, the MCP
// dispatcher), so none needs a key window.

private func rightArrow(option: Bool) -> NSEvent? {
    guard let scalar = UnicodeScalar(UInt32(NSRightArrowFunctionKey)) else { return nil }
    let characters = String(Character(scalar))
    return NSEvent.keyEvent(with: .keyDown, location: .zero,
                            modifierFlags: option ? [.option] : [],
                            timestamp: ProcessInfo.processInfo.systemUptime,
                            windowNumber: 0, context: nil, characters: characters,
                            charactersIgnoringModifiers: characters, isARepeat: false,
                            keyCode: 124)
}

/// One RGBA8 pixel of a canvas-sized layer at offset (0, 0).
private func rgba(_ pixels: [UInt8]?, width: Int, _ x: Int, _ y: Int) -> [UInt8] {
    guard let pixels else { return [] }
    let p = (y * width + x) * 4
    guard p + 4 <= pixels.count else { return [] }
    return Array(pixels[p..<(p + 4)])
}

private func offset(_ document: ImageDocument, _ layer: Int) -> [Int]? {
    document.doc?.layerInfo(layer).map { [$0.offsetX, $0.offsetY] }
}

private func samples(_ document: ImageDocument, _ points: [(Int, Int)]) throws -> [NSDictionary] {
    try points.map { NSDictionary(dictionary: try agentOK("sample_pixel", document,
                                                          ["x": $0.0, "y": $0.1])) }
}

private func depthFixture(_ depth: Int) throws -> ImageDocument {
    depth == 8 ? try fixture(width: 64, height: 64) : try precisionFixture(width: 64, height: 48)
}

func verifyAltNudgeDuplicate() throws {
    guard let optionRight = rightArrow(option: true), let plainRight = rightArrow(option: false) else {
        throw ProbeFailure(description: "Arrow key events")
    }
    // 8 bits, over a selection: the pixels duplicate, the selection moves.
    do {
        let document = try fixture(width: 64, height: 64)
        defer { document.close() }
        let controller = try editor(document)
        _ = try select(document, side: 16)
        controller.selectTool(.move)
        guard let base = document.doc, let before = base.layerPixels(0),
              let bounds = document.canvasSelection?.bounds else {
            throw ProbeFailure(description: "Nudge fixture")
        }
        controller.canvas.keyDown(with: optionRight)
        let after = document.doc?.layerPixels(0)
        try require(rgba(after, width: 64, 24, 30) == rgba(before, width: 64, 24, 30),
                    "(8) ⌥→ left the original at (24,30)")
        try require(rgba(after, width: 64, 40, 30) == rgba(before, width: 64, 39, 30),
                    "(8) ⌥→ landed P(39,30) at (40,30)")
        try require(document.canvasSelection?.bounds == bounds.offsetBy(dx: 1, dy: 0),
                    "(8) the selection moved +1: \(String(describing: document.canvasSelection?.bounds))")
        try require(document.undoManager?.undoActionName == "Duplicate",
                    "(8) one \"Duplicate\" step: \(document.undoManager?.undoActionName ?? "-")")
        document.undoManager?.undo()
        try require(document.doc === base, "(8) one undo restores the handle")
        controller.canvas.keyDown(with: plainRight)
        try require(rgba(document.doc?.layerPixels(0), width: 64, 24, 30).last == 0,
                    "(8) a plain → leaves the hole at (24,30)")
    }
    // 16 bits, over a selection: ⌥→ equals MCP move_selected_pixels duplicate.
    do {
        let document = try precisionFixture(width: 64, height: 48)
        defer { document.close() }
        let controller = try editor(document)
        _ = try select(document, side: 16)
        controller.selectTool(.move)
        guard let base = document.doc else { throw ProbeFailure(description: "16-bit nudge fixture") }
        controller.canvas.keyDown(with: optionRight)
        try require(document.doc !== base && document.undoManager?.undoActionName == "Duplicate",
                    "(16) ⌥→ is one \"Duplicate\" step")
        let points = [(24, 20), (40, 20)]
        let ui = try samples(document, points)
        document.undoManager?.undo()
        try require(document.doc === base, "(16) one undo restores the handle")
        _ = try agentOK("move_selected_pixels", document, ["dx": 1, "dy": 0, "duplicate": true])
        let agent = try samples(document, points)
        try require(ui == agent, "(16) ⌥→ equals MCP duplicate: \(ui) vs \(agent)")
    }
    // No selection: the layers duplicate and the copies move.
    for depth in [8, 16] {
        let document = try depthFixture(depth)
        defer { document.close() }
        let controller = try editor(document)
        controller.selectTool(.move)
        guard let base = document.doc else { throw ProbeFailure(description: "[\(depth)] layer nudge") }
        controller.canvas.keyDown(with: optionRight)
        try require(document.doc?.layerCount == 2, "[\(depth)] ⌥→ duplicated the layer")
        try require(document.doc?.layerInfo(1)?.name.hasSuffix("copy") == true,
                    "[\(depth)] layer 1 is the copy: \(document.doc?.layerInfo(1)?.name ?? "-")")
        try require(offset(document, 1) == [1, 0] && offset(document, 0) == [0, 0],
                    "[\(depth)] the copy moved, the original stayed: \(String(describing: offset(document, 1)))")
        try require(document.layerSelection.primary == 1,
                    "[\(depth)] the copy is the active layer: \(document.layerSelection.all)")
        try require(document.undoManager?.undoActionName == "Duplicate Layer",
                    "[\(depth)] one \"Duplicate Layer\" step: \(document.undoManager?.undoActionName ?? "-")")
        document.undoManager?.undo()
        try require(document.doc === base && document.doc?.layerCount == 1
                    && document.layerSelection.all == [0],
                    "[\(depth)] one undo gives one layer and selection [0]")
    }
    print("PASS: alt nudge duplicate — ⌥→ duplicates the selected pixels (\"Duplicate\", the "
        + "selection follows; 16 bits equals MCP) and with no selection the layers (\"Duplicate "
        + "Layer\", the copy moved and selected) at 8 and 16 bits")
}

func verifyAltDragDuplicatesLayers() throws {
    for depth in [8, 16] {
        let document = try depthFixture(depth)
        defer { document.close() }
        let controller = try editor(document)
        controller.selectTool(.move)
        guard let base = document.doc else { throw ProbeFailure(description: "[\(depth)] drag fixture") }

        controller.moveDidBegin(at: CGPoint(x: 10, y: 10), modifiers: [.option])
        controller.moveDidUpdate(5, 3, [.control])
        controller.moveDidFinish()
        try require(document.doc?.layerCount == 2, "[\(depth)] ⌥-drag duplicated the layer")
        try require(document.doc?.layerInfo(1)?.name.hasSuffix("copy") == true,
                    "[\(depth)] layer 1 is the copy")
        try require(offset(document, 1) == [5, 3] && offset(document, 0) == [0, 0],
                    "[\(depth)] the copy committed at (5,3): \(String(describing: offset(document, 1)))")
        try require(document.layerSelection.all == [1], "[\(depth)] the copy is selected: \(document.layerSelection.all)")
        try require(document.undoManager?.undoActionName == "Duplicate Layer",
                    "[\(depth)] one \"Duplicate Layer\" step: \(document.undoManager?.undoActionName ?? "-")")
        try require(controller.moveDuplicate == nil, "[\(depth)] the session is cleared")
        let steps = EditorViewController.moveRecord(base, [0], dx: 5, dy: 3, duplicate: true)
        try require(steps.first?.arguments["duplicate"] as? Bool == true,
                    "[\(depth)] moveRecord carries duplicate: \(String(describing: steps.first?.arguments))")
        document.undoManager?.undo()
        try require(document.doc === base && document.layerSelection.all == [0],
                    "[\(depth)] one undo gives one layer and selection [0]")

        // A press with no tick changes nothing.
        controller.moveDidBegin(at: CGPoint(x: 10, y: 10), modifiers: [.option])
        controller.moveDidFinish()
        try require(document.doc === base && controller.moveDuplicate == nil,
                    "[\(depth)] a click changed nothing")

        // A drag out and back leaves one in-place copy.
        controller.moveDidBegin(at: CGPoint(x: 10, y: 10), modifiers: [.option])
        controller.moveDidUpdate(4, 0, [.control])
        controller.moveDidUpdate(0, 0, [.control])
        controller.moveDidFinish()
        try require(document.doc?.layerCount == 2 && offset(document, 1) == [0, 0],
                    "[\(depth)] out and back leaves one in-place copy: \(String(describing: offset(document, 1)))")
        document.undoManager?.undo()
        try require(document.doc === base, "[\(depth)] undo after the in-place copy")

        // Cancel mid-drag clears the session.
        controller.moveDidBegin(at: CGPoint(x: 10, y: 10), modifiers: [.option])
        controller.moveDidUpdate(3, 0, [.control])
        controller.moveDidCancel()
        try require(controller.moveDuplicate == nil, "[\(depth)] cancel cleared the session")
        if document.doc !== base { document.undoManager?.undo() }
        try require(document.doc === base, "[\(depth)] back at the base after cancel")

        // MCP twin.
        let moved = try agentOK("move_layers", document,
                                ["layers": [0], "dx": 5, "dy": 3, "duplicate": true])
        try require((moved["copies"] as? [Int]) == [1] && moved["duplicate"] as? Bool == true,
                    "[\(depth)] move_layers duplicate answers copies [1]: \(moved)")
        try require(document.doc?.layerCount == 2 && offset(document, 1) == [5, 3]
                    && offset(document, 0) == [0, 0] && document.layerSelection.all == [1],
                    "[\(depth)] MCP gives the same state")
        document.undoManager?.undo()
        let zero = try agentOK("move_layers", document,
                               ["layers": [0], "dx": 0, "dy": 0, "duplicate": true])
        try require(document.doc?.layerCount == 2 && zero["changed"] as? Bool != false,
                    "[\(depth)] a zero-delta duplicate still duplicates: \(zero)")
        document.undoManager?.undo()
        let noop = try agentOK("move_layers", document, ["layers": [0], "dx": 0, "dy": 0])
        try require(noop["changed"] as? Bool == false && document.doc?.layerCount == 1,
                    "[\(depth)] without duplicate a zero delta is a no-op: \(noop)")
    }
    print("PASS: alt drag duplicates layers — ⌥-drag with no selection commits one \"Duplicate "
        + "Layer\" step with the copy at exactly the dragged offset and selected, at 8 and 16 bits; "
        + "a click changes nothing, out-and-back leaves an in-place copy, cancel clears the "
        + "session, and MCP move_layers duplicate matches")
}

func verifyFreeTransformCopy() throws {
    // 8 bits: translation and rotation against MCP.
    do {
        let document = try fixture(width: 64, height: 64)
        defer { document.close() }
        let controller = try editor(document)
        try require(controller.validateFreeTransformCopyItem(),
                    "enabled without a selection (it copies the layers)")
        _ = try select(document, side: 16)
        try require(controller.validateFreeTransformCopyItem(), "enabled with a selection")
        guard let base = document.doc, let before = base.layerPixels(0) else {
            throw ProbeFailure(description: "⌥⌘T fixture")
        }
        let sampler = EditorViewController.samplerAgentName(controller.transformSampler)
        controller.freeTransformCopy(nil)
        try require(controller.transformSession?.floating?.duplicate == true, "⌥⌘T opened a copy session")
        controller.transformSession?.transform.translation = CGVector(dx: 7, dy: 0)
        _ = controller.commitTransformSession()
        try require(rgba(document.doc?.layerPixels(0), width: 64, 24, 30) == rgba(before, width: 64, 24, 30),
                    "(8) the original stays at (24,30)")
        try require(document.undoManager?.undoActionName == "Transform Copy",
                    "(8) one \"Transform Copy\" step: \(document.undoManager?.undoActionName ?? "-")")
        let ui = document.doc?.layerPixels(0)
        document.undoManager?.undo()
        try require(document.doc === base, "(8) undo restores the base")
        _ = try agentOK("transform_selected_pixels", document,
                        ["translate_x": 7, "duplicate": true, "sampler": sampler])
        try require(document.doc?.layerPixels(0) == ui, "(8) MCP translate_x 7 duplicate equals ⌥⌘T")
        document.undoManager?.undo()

        controller.freeTransformCopy(nil)
        controller.transformSession?.transform.degrees = 30
        _ = controller.commitTransformSession()
        let rotated = document.doc?.layerPixels(0)
        try require(rotated != nil && rotated != before, "(8) the rotated copy landed")
        document.undoManager?.undo()
        try require(document.doc === base, "(8) undo restores the base after the rotation")
        _ = try agentOK("transform_selected_pixels", document,
                        ["rotate": 30, "duplicate": true, "sampler": sampler])
        try require(document.doc?.layerPixels(0) == rotated, "(8) MCP rotate 30 duplicate equals ⌥⌘T")
    }
    // 16 bits: rotation against MCP at six points.
    do {
        let document = try precisionFixture(width: 64, height: 48)
        defer { document.close() }
        let controller = try editor(document)
        _ = try select(document, side: 16)
        guard let base = document.doc else { throw ProbeFailure(description: "16-bit ⌥⌘T fixture") }
        let sampler = EditorViewController.samplerAgentName(controller.transformSampler)
        controller.freeTransformCopy(nil)
        try require(controller.transformSession?.floating?.duplicate == true, "(16) copy session")
        controller.transformSession?.transform.degrees = 30
        _ = controller.commitTransformSession()
        try require(document.doc !== base && document.undoManager?.undoActionName == "Transform Copy",
                    "(16) one \"Transform Copy\" step")
        let points = [(24, 20), (30, 24), (35, 30), (20, 16), (40, 28), (28, 34)]
        let ui = try samples(document, points)
        document.undoManager?.undo()
        try require(document.doc === base, "(16) undo restores the base")
        _ = try agentOK("transform_selected_pixels", document,
                        ["rotate": 30, "duplicate": true, "sampler": sampler])
        let agent = try samples(document, points)
        try require(ui == agent, "(16) MCP rotate 30 duplicate equals ⌥⌘T: \(ui) vs \(agent)")
    }
    print("PASS: free transform copy — ⌥⌘T validates with and without a selection, keeps the original, commits "
        + "one \"Transform Copy\" step and equals MCP transform_selected_pixels duplicate for a "
        + "translation and a rotation at 8 and 16 bits")
}

/// A 64×64 document opaque only in (20, 20, 10, 10).
private func sparseFixture() throws -> ImageDocument {
    var bytes = [UInt8](repeating: 0, count: 64 * 64 * 4)
    for y in 20..<30 {
        for x in 20..<30 {
            let p = (y * 64 + x) * 4
            bytes[p] = 200
            bytes[p + 1] = 100
            bytes[p + 2] = 50
            bytes[p + 3] = 255
        }
    }
    guard let image = RasterImage.from(rgba: bytes, width: 64, height: 64),
          let document = ImageDocument.makeUntitled(with: image) else {
        throw ProbeFailure(description: "Sparse fixture")
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

/// The "Reveal Probe" panel's alpha at each of `times` after entry, and the
/// largest alpha seen by a 10 ms poll; -1 when no panel is on screen.
private func revealReadings(task seconds: TimeInterval, revealAfter: TimeInterval,
                            times: [TimeInterval]) throws -> (at: [CGFloat], max: CGFloat) {
    func alpha() -> CGFloat {
        NSApp.windows.first { $0.title == "Reveal Probe" && $0.isVisible }?.alphaValue ?? -1
    }
    var readings = [CGFloat](repeating: -2, count: times.count)
    var largest: CGFloat = -1
    var timers: [Timer] = []
    for (index, time) in times.enumerated() {
        timers.append(Timer(timeInterval: time, repeats: false) { _ in readings[index] = alpha() })
    }
    timers.append(Timer(timeInterval: 0.01, repeats: true) { _ in largest = max(largest, alpha()) })
    for timer in timers {
        RunLoop.main.add(timer, forMode: .common)
        RunLoop.main.add(timer, forMode: .modalPanel)
    }
    defer { timers.forEach { $0.invalidate() } }
    let task = try WorkerTask<Int> {
        Thread.sleep(forTimeInterval: seconds)
        return 1
    }
    _ = try BackgroundWork.wait(task, title: "Reveal Probe", revealAfter: revealAfter)
    return (readings, largest)
}

func verifySelectedPixelsPolish() throws {
    // (a) The ⌥ cursor.
    do {
        let document = try fixture(width: 64, height: 64)
        defer { document.close() }
        let controller = try editor(document)
        controller.selectTool(.move)
        let canvas: ImageCanvasView = controller.canvas
        canvas.selectionCursorModifiers = [.option]
        try require(canvas.selectionFeedbackCursor(latchedMode: nil) == NSCursor.dragCopy,
                    "(a) ⌥ over the Move tool shows the copy cursor")
        canvas.selectionCursorModifiers = []
        try require(canvas.selectionFeedbackCursor(latchedMode: nil) == NSCursor.openHand,
                    "(a) the Move tool rests on the open hand")
        canvas.moveDragDuplicate = true
        try require(canvas.moveDragCursor == NSCursor.dragCopy, "(a) an ⌥ drag shows the copy cursor")
        canvas.moveDragDuplicate = false
        try require(canvas.moveDragCursor == NSCursor.closedHand, "(a) a plain drag closes the hand")
    }
    // (b) The drag snaps to the float's opaque bounds once the query answers.
    do {
        let document = try sparseFixture()
        defer { document.close() }
        let controller = try editor(document)
        controller.selectTool(.move)
        document.setCanvasSelection(CanvasSelection(
            shape: .rect(CGRect(x: 10, y: 10, width: 40, height: 40)), canvasWidth: 64, canvasHeight: 64))
        controller.moveDidBegin(at: CGPoint(x: 30, y: 30), modifiers: [])
        try require(controller.movePressBox == CGRect(x: 10, y: 10, width: 40, height: 40),
                    "(b) the press box is selection ∩ layer: \(String(describing: controller.movePressBox))")
        controller.moveDidUpdate(1, 0, [.control])
        try pumpUntil({ controller.movePressBox == CGRect(x: 20, y: 20, width: 10, height: 10) },
                      seconds: 5)
        controller.moveDidCancel()
    }
    // (c) The delayed-reveal wait.
    let delayed = try revealReadings(task: 0.6, revealAfter: 0.25, times: [0.1, 0.45])
    try require(delayed.at[0] == 0 && delayed.at[1] == 1,
                "(c) hidden at 0.1 s and shown at 0.45 s: \(delayed.at)")
    let immediate = try revealReadings(task: 0.6, revealAfter: 0, times: [0.1])
    try require(immediate.at[0] == 1, "(c) revealAfter 0 shows at once: \(immediate.at)")
    let short = try revealReadings(task: 0.05, revealAfter: 0.25, times: [])
    try require(short.max < 1, "(c) a 0.05 s wait never shows its panel: \(short.max)")
    print("PASS: selected pixels polish — the Move tool's ⌥ cursors, the drag snapping to the "
        + "float's opaque bounds, and a delayed-reveal wait that stays hidden until its delay")
}
