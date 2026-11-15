import AppKit

// The Background Eraser tool in the UI (`designs/background-eraser.md`,
// EditorViewController+BackgroundEraser.swift):
//
// (a) Its facts: the name, a symbol that resolves, the Eraser's rail slot right after the Magic
//     Eraser with distinct glyphs, E cycling Eraser → Magic Eraser → Background Eraser → Eraser, a
//     brush tip read as coverage, and its stored tip with opacity, flow and airbrush pinned.
// (b) Every mode's gate: offered in RGB at 8, 16 and 32 bits and in Grayscale, Lab, CMYK (8 and 16
//     bits) and Indexed; not in Bitmap.
// (c) A mouse-down, drags and a mouse-up handed to the canvas view (its own handlers, at the
//     window points of image pixels), at 8 and 16 bits, over a green field with a red row under
//     the tip, Tolerance 0, Contiguous, Continuous: only green pixels lose alpha (their colour
//     kept), the red row and everything else stay exactly, as ONE "Background Eraser" step that
//     records `background_eraser_stroke` (no flow or opacity key); one undo restores the layer.
//     With the layer's mask as the target the mouse-down refuses by name and nothing changes.
// (d) The swatches' stored CMYK inks are app-wide and outlive a switch to an RGB document: a
//     stroke there with Background Swatch sampling and Protect Foreground, both swatches
//     carrying an ink, records `sample_color` and `protect_color` and neither `_ink` key (a
//     replay on that document would refuse one).

func verifyBackgroundEraser() throws {
    // Every part runs, so one run names each part that fails.
    var failures: [String] = []
    for part in [verifyBackgroundEraserFacts, { try verifyBackgroundEraserStroke(sixteen: false) },
                 { try verifyBackgroundEraserStroke(sixteen: true) },
                 verifyBackgroundEraserStoredInks] {
        do { try part() } catch { failures.append(String(describing: error)) }
    }
    try require(failures.isEmpty, failures.joined(separator: " ;; "))
    print("PASS: the Background Eraser's facts, its gate in every mode, its stroke at 8 and "
          + "16 bits, and no stored ink recorded on an RGB document")
}

private func verifyBackgroundEraserFacts() throws {
    let tag = "[background eraser facts]"
    let tool = EditorTool.backgroundEraser
    try require(tool.displayName == "Background Eraser" && tool.keyCharacter == "e"
                && NSImage(systemSymbolName: tool.symbol, accessibilityDescription: nil) != nil,
                "\(tag) the name, key and symbol: \(tool.displayName) \(tool.keyCharacter) \(tool.symbol)")
    try require(tool.usesBrushTip && tool.strokeIsCoverage && !tool.acceptsMarginPress
                && tool.minimumBrushSize == 1 && tool.maximumBrushSize == 1000,
                "\(tag) a brush tip read as coverage, 1–1000 px")
    let slot = EditorTool.railGroups[EditorTool.eraser.railGroupIndex]
    try require(slot.firstIndex(of: .backgroundEraser) == slot.firstIndex(of: .magicEraser).map { $0 + 1 },
                "\(tag) the Background Eraser follows the Magic Eraser in its slot: \(slot)")
    let glyphs = slot.map(\.fallbackGlyph)
    try require(Set(glyphs).count == glyphs.count, "\(tag) the slot's glyphs are distinct: \(glyphs)")
    try require(EditorTool.tool(forKey: "e", current: .eraser, last: nil) == .magicEraser
                && EditorTool.tool(forKey: "e", current: .magicEraser, last: nil) == .backgroundEraser
                && EditorTool.tool(forKey: "e", current: .backgroundEraser, last: nil) == .eraser,
                "\(tag) E cycles the three erasers")
    let store = ToolOptionsStore.shared
    let saved = store.backgroundEraser
    defer { store.backgroundEraser = saved }
    var bent = PaintToolOptions(size: 30, hardness: 60, opacity: 40, flow: 30)
    bent.airbrush = true
    store.setPaintOptions(bent, for: .backgroundEraser)
    guard let read = store.paintOptions(for: .backgroundEraser) else {
        throw ProbeFailure(description: "\(tag) the tool has paint options")
    }
    try require(read.size == 30 && read.hardness == 60 && read.opacity == 100 && read.flow == 100
                && !read.airbrush, "\(tag) the tip keeps, opacity and flow stay pinned: \(read)")
    // (b) The mode gates.
    try require(GrayDocument.allows(.backgroundEraser) && LabDocument.allows(.backgroundEraser)
                && IndexedDocument.allows(.backgroundEraser)
                && CmykDocument.allows(.backgroundEraser, depth: 8)
                && CmykDocument.allows(.backgroundEraser, depth: 16)
                && !BitmapDocument.allows(.backgroundEraser),
                "\(tag) offered in Gray, Lab, Indexed and CMYK, not in Bitmap")
    for (name, make) in [("8-bit", { try fixture(width: 24, height: 16) }),
                         ("16-bit", { try precisionFixture(width: 24, height: 16) }),
                         ("float", { try floatFixture() })] as [(String, () throws -> ImageDocument)] {
        let document = try make()
        defer { document.close() }
        try require(DocumentPrecision.allows(.backgroundEraser, document: document),
                    "\(tag) offered on a \(name) RGB document")
    }
}

/// Image pixel `point` as a window location on `canvas`.
private func windowLocation(_ canvas: ImageCanvasView, _ point: CGPoint) -> NSPoint {
    canvas.convert(NSPoint(x: point.x, y: point.y), to: nil)
}

private func mouse(_ canvas: ImageCanvasView, _ type: NSEvent.EventType, at point: CGPoint,
                   tag: String) throws {
    guard let window = canvas.window, let event = NSEvent.mouseEvent(
        with: type, location: windowLocation(canvas, point), modifierFlags: [],
        timestamp: ProcessInfo.processInfo.systemUptime, windowNumber: window.windowNumber,
        context: nil, eventNumber: 1, clickCount: 1, pressure: 1) else {
        throw ProbeFailure(description: "\(tag) could not create the \(type) event")
    }
    switch type {
    case .leftMouseDown: canvas.mouseDown(with: event)
    case .leftMouseDragged: canvas.mouseDragged(with: event)
    default: canvas.mouseUp(with: event)
    }
}

/// The layer's (red, alpha) at every pixel: bytes at 8 bits, u16 codes at 16.
private func redAlpha(_ doc: RasterDocument?, sixteen: Bool) throws -> [[Int]] {
    if sixteen {
        guard let doc else { throw ProbeFailure(description: "no 16-bit document") }
        let red = try PrecisionScalarOperations.read(doc, target: RasterPlane.red.agentName, index: 0)
            .samples.map(Int.init)
        let alpha = try PrecisionScalarOperations.read(doc, target: RasterPlane.alpha.agentName,
                                                       index: 0).samples.map(Int.init)
        return zip(red, alpha).map { [$0, $1] }
    }
    guard let pixels = doc?.layerPixels(0) else { throw ProbeFailure(description: "no pixels") }
    return stride(from: 0, to: pixels.count, by: 4).map { [Int(pixels[$0]), Int(pixels[$0 + 3])] }
}

private func verifyBackgroundEraserStroke(sixteen: Bool) throws {
    let tag = "[background eraser stroke \(sixteen ? 16 : 8)]"
    let width = 24, height = 16
    let document = try sixteen ? precisionFixture(width: width, height: height)
        : fixture(width: width, height: height)
    defer {
        if ActionRecorder.shared.isRecording { _ = ActionRecorder.shared.stop() }
        document.close()
    }
    let controller = try editor(document)
    let canvas: ImageCanvasView = controller.canvas
    // A green field (x 4–19, y 3–8) and a red row under it (y 9) on the noisy fixture.
    _ = try agentOK("select_rect", document, ["x": 4, "y": 3, "width": 16, "height": 6])
    _ = try agentOK("fill_selection", document, ["color": "#20a040"])
    _ = try agentOK("select_rect", document, ["x": 4, "y": 9, "width": 16, "height": 1])
    _ = try agentOK("fill_selection", document, ["color": "#c02020"])
    _ = try agentOK("deselect", document)
    document.undoManager?.removeAllActions()
    guard let base = document.doc else { throw ProbeFailure(description: "\(tag) no base") }
    let before = try redAlpha(base, sixteen: sixteen)
    let green = before[5 * width + 10][0], red = before[9 * width + 10][0]
    let store = ToolOptionsStore.shared
    let (savedTip, savedOptions) = (store.backgroundEraser, store.backgroundEraserOptions)
    defer {
        store.backgroundEraser = savedTip
        store.backgroundEraserOptions = savedOptions
    }
    var tip = PaintToolOptions(size: 7, smoothing: 0)
    tip.pressureSize = false
    store.setPaintOptions(tip, for: .backgroundEraser)
    store.backgroundEraserOptions = BackgroundEraserToolOptions(
        tolerance: 0, samplingIndex: 0, limitsIndex: 0, protectForeground: false)
    controller.selectTool(.backgroundEraser)
    try require(controller.currentTool == .backgroundEraser && canvas.tool == .backgroundEraser
                && canvas.brushSize == 7, "\(tag) the Background Eraser is selected at 7 px")
    let entries = document.history.entries.count
    let steps = try uiRecorded {
        try mouse(canvas, .leftMouseDown, at: CGPoint(x: 8.5, y: 6.5), tag: tag)
        for x in [10.5, 12.5, 14.5] {
            try mouse(canvas, .leftMouseDragged, at: CGPoint(x: x, y: 6.5), tag: tag)
        }
        try mouse(canvas, .leftMouseUp, at: CGPoint(x: 15.5, y: 6.5), tag: tag)
        if sixteen {
            guard let job = DocumentJobs.active else {
                throw ProbeFailure(description: "\(tag) the stroke started no job")
            }
            try pumpUntil({ job.state != "running" })
            try require(job.state == "completed", "\(tag) the stroke's job completed: \(job.report)")
        }
    }
    guard let erased = document.doc, erased !== base else {
        throw ProbeFailure(description: "\(tag) the stroke published nothing")
    }
    let after = try redAlpha(erased, sixteen: sixteen)
    var cleared = 0
    for y in 0..<height {
        for x in 0..<width {
            let i = y * width + x
            let (b, a) = (before[i], after[i])
            guard a != b else { continue }
            let isGreen = (4..<20).contains(x) && (3..<9).contains(y)
            try require(isGreen && a[0] == b[0] && a[1] < b[1],
                        "\(tag) (\(x), \(y)) changed: \(b) → \(a); only green alpha may fall")
            if a[1] == 0 { cleared += 1 }
        }
    }
    try require(cleared > 30, "\(tag) the green under the stroke clears: \(cleared) pixels")
    try require((4..<20).allSatisfy { after[9 * width + $0] == before[9 * width + $0] }
                && green != red, "\(tag) the red row under the tip stays")
    try require(document.history.entries.count == entries + 1
                && document.undoManager?.undoActionName == "Background Eraser",
                "\(tag) one \"Background Eraser\" step: \(document.history.entries.map(\.name))")
    try require(steps.map(\.tool) == ["background_eraser_stroke"]
                && steps.first?.arguments["tolerance"] as? Int == 0
                && steps.first?.arguments["native_stroke"] != nil
                && steps.first?.arguments["flow"] == nil && steps.first?.arguments["opacity"] == nil,
                "\(tag) the stroke records background_eraser_stroke: \(steps.map(\.arguments))")
    document.undoManager?.undo()
    try require(document.doc === base, "\(tag) one undo restores the layer")

    // The layer's mask as the target refuses by name and changes nothing.
    _ = try agentOK("add_layer_mask", document, ["layer": 0])
    document.undoManager?.removeAllActions()
    controller.setPaintTarget(.mask)
    let masked = document.doc
    AutomationAlerts.reset()
    try mouse(canvas, .leftMouseDown, at: CGPoint(x: 8.5, y: 6.5), tag: tag)
    if canvas.strokeActive { try mouse(canvas, .leftMouseUp, at: CGPoint(x: 8.5, y: 6.5), tag: tag) }
    let said = AutomationAlerts.recorded.map(\.messageText)
    AutomationAlerts.reset()
    try require(said == [BackgroundEraser.targetRefusal] && document.doc === masked,
                "\(tag) a mask target refuses by name: \(said)")
    controller.setPaintTarget(.layer)
}

/// (d): an 8-bit RGB stroke with both swatches carrying a stored CMYK ink.
private func verifyBackgroundEraserStoredInks() throws {
    let tag = "[background eraser stored inks]"
    let document = try fixture(width: 24, height: 16)
    defer {
        if ActionRecorder.shared.isRecording { _ = ActionRecorder.shared.stop() }
        document.close()
    }
    let controller = try editor(document)
    let canvas: ImageCanvasView = controller.canvas
    _ = try agentOK("select_rect", document, ["x": 4, "y": 3, "width": 16, "height": 6])
    _ = try agentOK("fill_selection", document, ["color": "#20a040"])
    _ = try agentOK("deselect", document)
    let store = ToolOptionsStore.shared
    let (savedTip, savedOptions) = (store.backgroundEraser, store.backgroundEraserOptions)
    let (savedForeground, savedBackground) = (controller.paintColor, controller.backgroundColor)
    defer {
        store.backgroundEraser = savedTip
        store.backgroundEraserOptions = savedOptions
        controller.setPaintColor(savedForeground)
        controller.setBackgroundColor(savedBackground)
    }
    // The inks a CMYK document's Color Picker leaves: a blue foreground (nothing here is like
    // it) and the field's green as the background, which the swatch sampling erases.
    controller.setPaintInk(CmykInk(c: 100, m: 80, y: 0, k: 0),
                           color: NSColor(srgbRed: 0, green: 0, blue: 1, alpha: 1))
    controller.setBackgroundInk(CmykInk(c: 80, m: 0, y: 100, k: 10),
                                color: NSColor(srgbRed: 0x20 / 255.0, green: 0xa0 / 255.0,
                                               blue: 0x40 / 255.0, alpha: 1))
    try require(store.sharedState.foregroundInk != nil && store.sharedState.backgroundInk != nil,
                "\(tag) both swatches carry an ink")
    var tip = PaintToolOptions(size: 7, smoothing: 0)
    tip.pressureSize = false
    store.setPaintOptions(tip, for: .backgroundEraser)
    store.backgroundEraserOptions = BackgroundEraserToolOptions(
        tolerance: 32, samplingIndex: 2, limitsIndex: 0, protectForeground: true)
    controller.selectTool(.backgroundEraser)
    let base = document.doc
    let steps = try uiRecorded {
        try mouse(canvas, .leftMouseDown, at: CGPoint(x: 8.5, y: 6.5), tag: tag)
        try mouse(canvas, .leftMouseDragged, at: CGPoint(x: 12.5, y: 6.5), tag: tag)
        try mouse(canvas, .leftMouseUp, at: CGPoint(x: 14.5, y: 6.5), tag: tag)
    }
    try require(document.doc !== base, "\(tag) the stroke erased the green")
    let arguments = steps.first?.arguments ?? [:]
    try require(steps.map(\.tool) == ["background_eraser_stroke"]
                && arguments["sampling"] as? String == "background_swatch"
                && arguments["sample_color"] != nil && arguments["protect_color"] != nil
                && arguments["sample_ink"] == nil && arguments["protect_ink"] == nil,
                "\(tag) the colours are recorded and the stored inks are not: "
                    + "\(steps.map(\.tool)) \(arguments.keys.sorted())")
}
