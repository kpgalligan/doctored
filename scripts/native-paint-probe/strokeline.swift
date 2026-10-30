import AppKit

/// One ⇧-press + release at `point` through the actual canvas handlers.
private func shiftClick(_ canvas: ImageCanvasView, _ point: CGPoint) throws {
    try send(canvas, .leftMouseDown, point, modifiers: [.shift])
    try send(canvas, .leftMouseUp, point, modifiers: [.shift])
    pump()
}

/// The straight-RGBA8 composite's pixel at (x, y).
private func compositePixel(_ bytes: [UInt8], x: Int, y: Int, width: Int) -> [UInt8] {
    let i = (y * width + x) * 4
    return Array(bytes[i..<(i + 4)])
}

/// A ⇧-click with a stroke tool paints a straight segment from where the last stroke
/// ended, as one ordinary stroke (its own undo step and recorded Action); with no prior stroke it
/// is a plain click. The anchor clears on a real tool change and survives a same-tool reselect.
func shiftClickLineUI() throws -> [String: Any] {
    let (document, editor, source) = try openBytePaintDocument(opaque: true)
    defer {
        if ActionRecorder.shared.isRecording { _ = ActionRecorder.shared.stop() }
        document.close(); pump()
    }
    let canvas = editor.canvas
    let width = source.width
    configure(editor, size: 12, hardness: 1, opacity: 100, flow: 100)
    // Smoothing on so the leash is live: the chord must bypass it.
    ToolOptionsStore.shared.brush.smoothing = 60
    editor.syncCanvasPaintState(); pump()
    document.undoManager?.removeAllActions()
    ActionRecorder.shared.start()
    let fixture = try flatBytes(source)

    // a. No anchor yet: a ⇧-click is a plain click.
    let a = CGPoint(x: 20.5, y: 20.5)
    let history0 = document.history.entries.count
    try require(canvas.strokeLineAnchor == nil, "Fresh canvas already had a stroke-line anchor")
    try shiftClick(canvas, a)
    try require(canvas.recordedStrokePoints == [a],
        "⇧-click without an anchor was not a plain click: \(canvas.recordedStrokePoints)")
    try require(canvas.strokeLineAnchor == a, "Plain click did not latch the stroke-line anchor")
    try require(document.history.entries.count == history0 + 1, "Plain click was not one history step")

    // b. ⇧-click with an anchor: an exact straight chord, leash bypassed.
    let b = CGPoint(x: 76.5, y: 44.5)
    try shiftClick(canvas, b)
    try require(canvas.recordedStrokePoints == [a, b],
        "⇧-click did not record the exact chord [A, B]: \(canvas.recordedStrokePoints)")
    try require(canvas.strokeLineAnchor == b, "⇧-click did not move the anchor to its end")
    try require(document.history.entries.count == history0 + 2, "⇧-click line was not one history step")
    guard let lined = document.doc else { throw DoctoredCoreError(message: "Lined document missing") }
    let linedBytes = try flatBytes(lined)
    try require(compositePixel(linedBytes, x: 48, y: 32, width: width)
        != compositePixel(fixture, x: 48, y: 32, width: width),
        "⇧-click line left its midpoint unpainted")
    try require(compositePixel(linedBytes, x: 20, y: 50, width: width)
        == compositePixel(fixture, x: 20, y: 50, width: width),
        "⇧-click line painted off the chord")
    let steps = ActionRecorder.shared.steps
    try require(steps.count == 2 && steps.allSatisfy { $0.tool == "brush_stroke" },
        "Expected two recorded brush_stroke steps, got \(steps.map(\.tool))")

    // c. The line is its own undo step.
    document.undoManager?.undo(); pump()
    guard let undone = document.doc else { throw DoctoredCoreError(message: "Undone document missing") }
    let undoneBytes = try flatBytes(undone)
    try require(compositePixel(undoneBytes, x: 48, y: 32, width: width)
        == compositePixel(fixture, x: 48, y: 32, width: width),
        "Undo did not remove the ⇧-click line")
    try require(compositePixel(undoneBytes, x: 20, y: 20, width: width)
        != compositePixel(fixture, x: 20, y: 20, width: width),
        "Undo of the line also removed the first click")
    document.undoManager?.redo(); pump()
    try require(document.doc === lined, "Redo did not restore the ⇧-click line")

    // d. A real tool change clears the anchor.
    editor.selectTool(.eraser); pump()
    try require(canvas.strokeLineAnchor == nil, "Tool change kept the stroke-line anchor")
    var eraser = PaintToolOptions()
    eraser.size = 12; eraser.hardness = 100; eraser.opacity = 100; eraser.flow = 100
    ToolOptionsStore.shared.eraser = eraser
    editor.syncCanvasPaintState(); pump()
    let c = CGPoint(x: 48.5, y: 32.5)
    try shiftClick(canvas, c)
    try require(canvas.recordedStrokePoints.count == 1,
        "Eraser ⇧-click after a tool change was not a plain click")
    try shiftClick(canvas, a)
    try require(canvas.recordedStrokePoints == [c, a],
        "Eraser ⇧-click did not record the chord [C, A]: \(canvas.recordedStrokePoints)")
    guard let erased = document.doc else { throw DoctoredCoreError(message: "Erased document missing") }
    let erasedBytes = try flatBytes(erased)
    try require(compositePixel(erasedBytes, x: 34, y: 26, width: width)[3] < 255,
        "Eraser ⇧-click line did not erase along the chord")
    try require(compositePixel(erasedBytes, x: 70, y: 42, width: width)[3] == 255,
        "Eraser ⇧-click line erased off the chord")

    // e. A same-tool reselect keeps the anchor.
    editor.selectTool(.eraser); pump()
    try require(canvas.strokeLineAnchor == a, "Same-tool reselect cleared the stroke-line anchor")
    let recorded = ActionRecorder.shared.stop()
    return ["case": "shift-click-line-ui", "passed": true,
            "recorded_tools": recorded.map(\.tool),
            "line_composite": hashBytes(linedBytes), "erased_composite": hashBytes(erasedBytes)]
}
