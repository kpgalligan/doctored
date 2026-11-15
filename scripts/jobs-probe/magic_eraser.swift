import AppKit

// The Magic Eraser tool in the UI (`designs/magic-eraser.md`, EditorViewController+MagicEraser.swift):
//
// (a) Its facts: the name, a symbol that resolves, the Eraser's rail slot right after the Eraser,
//     and E, which cycles the Eraser and the Magic Eraser.
// (b) Every mode's gate: offered in RGB at 8, 16 and 32 bits and in Grayscale, Lab, CMYK (8 and 16
//     bits) and Indexed; not in Bitmap.
// (c) A mouse-down handed to the canvas view (its own `mouseDown`, at the window point of an image
//     pixel), at 8 and 16 bits: with Tolerance 0 the uniform square under the click goes clear and
//     nothing else moves, as ONE "Magic Eraser" step that records `magic_eraser`; one undo
//     restores the layer. With the layer's mask as the target the click refuses by name and
//     nothing changes.

func verifyMagicEraser() throws {
    // Every part runs, so one run names each part that fails.
    var failures: [String] = []
    for part in [verifyMagicEraserFacts, { try verifyMagicEraserClick(sixteen: false) },
                 { try verifyMagicEraserClick(sixteen: true) }] {
        do { try part() } catch { failures.append(String(describing: error)) }
    }
    try require(failures.isEmpty, failures.joined(separator: " ;; "))
    print("PASS: the Magic Eraser's facts, its gate in every mode, and its click at 8 and 16 bits")
}

private func verifyMagicEraserFacts() throws {
    let tag = "[magic eraser facts]"
    let tool = EditorTool.magicEraser
    try require(tool.displayName == "Magic Eraser" && tool.keyCharacter == "e"
                && NSImage(systemSymbolName: tool.symbol, accessibilityDescription: nil) != nil,
                "\(tag) the name, key and symbol: \(tool.displayName) \(tool.keyCharacter) \(tool.symbol)")
    let slot = EditorTool.railGroups[EditorTool.eraser.railGroupIndex]
    try require(slot.firstIndex(of: .magicEraser) == slot.firstIndex(of: .eraser).map { $0 + 1 },
                "\(tag) the Magic Eraser follows the Eraser in its slot: \(slot)")
    let glyphs = slot.map(\.fallbackGlyph)
    try require(Set(glyphs).count == glyphs.count, "\(tag) the slot's glyphs are distinct: \(glyphs)")
    try require(EditorTool.tool(forKey: "e", current: .eraser, last: nil) == .magicEraser
                && EditorTool.tool(forKey: "e", current: .magicEraser, last: nil) == .eraser,
                "\(tag) E cycles the Eraser and the Magic Eraser")
    // (b) The mode gates.
    try require(GrayDocument.allows(.magicEraser) && LabDocument.allows(.magicEraser)
                && IndexedDocument.allows(.magicEraser)
                && CmykDocument.allows(.magicEraser, depth: 8)
                && CmykDocument.allows(.magicEraser, depth: 16)
                && !BitmapDocument.allows(.magicEraser),
                "\(tag) offered in Gray, Lab, Indexed and CMYK, not in Bitmap")
    for (name, make) in [("8-bit", { try fixture(width: 24, height: 16) }),
                         ("16-bit", { try precisionFixture(width: 24, height: 16) }),
                         ("float", { try floatFixture() })] as [(String, () throws -> ImageDocument)] {
        let document = try make()
        defer { document.close() }
        try require(DocumentPrecision.allows(.magicEraser, document: document),
                    "\(tag) offered on a \(name) RGB document")
    }
}

/// The image pixel `point`'s centre as a window location on `canvas`.
private func windowLocation(_ canvas: ImageCanvasView, _ point: CGPoint) -> NSPoint {
    canvas.convert(NSPoint(x: point.x + 0.5, y: point.y + 0.5), to: nil)
}

private func click(_ canvas: ImageCanvasView, at point: CGPoint, tag: String) throws {
    guard let window = canvas.window, let down = NSEvent.mouseEvent(
        with: .leftMouseDown, location: windowLocation(canvas, point), modifierFlags: [],
        timestamp: ProcessInfo.processInfo.systemUptime, windowNumber: window.windowNumber,
        context: nil, eventNumber: 1, clickCount: 1, pressure: 1) else {
        throw ProbeFailure(description: "\(tag) could not create the mouse-down")
    }
    canvas.mouseDown(with: down)
}

/// The layer's alpha at every pixel (u8 at 8 bits, u16 codes at 16).
private func alphas(_ doc: RasterDocument?, sixteen: Bool) throws -> [Int] {
    if sixteen {
        guard let doc else { throw ProbeFailure(description: "no 16-bit document") }
        return try PrecisionScalarOperations.read(doc, target: RasterPlane.alpha.agentName, index: 0)
            .samples.map(Int.init)
    }
    guard let pixels = doc?.layerPixels(0) else { throw ProbeFailure(description: "no pixels") }
    return stride(from: 3, to: pixels.count, by: 4).map { Int(pixels[$0]) }
}

private func verifyMagicEraserClick(sixteen: Bool) throws {
    let tag = "[magic eraser click \(sixteen ? 16 : 8)]"
    let width = 24, height = 16
    let document = try sixteen ? precisionFixture(width: width, height: height)
        : fixture(width: width, height: height)
    defer {
        if ActionRecorder.shared.isRecording { _ = ActionRecorder.shared.stop() }
        document.close()
    }
    let controller = try editor(document)
    let canvas: ImageCanvasView = controller.canvas
    // A uniform square (x 4–9, y 3–8) on the noisy fixture.
    _ = try agentOK("select_rect", document, ["x": 4, "y": 3, "width": 6, "height": 6])
    _ = try agentOK("fill_selection", document, ["color": "#20a040"])
    _ = try agentOK("deselect", document)
    document.undoManager?.removeAllActions()
    guard let base = document.doc else { throw ProbeFailure(description: "\(tag) no base") }
    let before = try alphas(base, sixteen: sixteen)
    let store = ToolOptionsStore.shared
    let saved = store.magicEraser
    defer { store.magicEraser = saved }
    store.magicEraser = MagicEraserToolOptions(tolerance: 0, contiguous: true,
                                               sampleAllLayers: false, opacity: 100)
    controller.selectTool(.magicEraser)
    try require(controller.currentTool == .magicEraser && canvas.tool == .magicEraser,
                "\(tag) the Magic Eraser is selected")
    let entries = document.history.entries.count
    let steps = try uiRecorded { try click(canvas, at: CGPoint(x: 6, y: 5), tag: tag) }
    guard let erased = document.doc, erased !== base else {
        throw ProbeFailure(description: "\(tag) the click published nothing")
    }
    let after = try alphas(erased, sixteen: sixteen)
    for y in 0..<height {
        for x in 0..<width {
            let i = y * width + x
            let inSquare = (4..<10).contains(x) && (3..<9).contains(y)
            try require(inSquare ? after[i] == 0 : after[i] == before[i],
                        "\(tag) (\(x), \(y)) in the square \(inSquare): \(before[i]) → \(after[i])")
        }
    }
    try require(document.history.entries.count == entries + 1
                && document.undoManager?.undoActionName == "Magic Eraser",
                "\(tag) one \"Magic Eraser\" step: \(document.history.entries.map(\.name))")
    try require(steps.map(\.tool) == ["magic_eraser"]
                && steps.first?.arguments["tolerance"] as? Int == 0,
                "\(tag) the click records magic_eraser: \(steps.map(\.tool))")
    document.undoManager?.undo()
    try require(document.doc === base, "\(tag) one undo restores the layer")

    // The layer's mask as the target refuses by name and changes nothing.
    _ = try agentOK("add_layer_mask", document, ["layer": 0])
    document.undoManager?.removeAllActions()
    controller.setPaintTarget(.mask)
    let masked = document.doc
    AutomationAlerts.reset()
    try click(canvas, at: CGPoint(x: 6, y: 5), tag: tag)
    let said = AutomationAlerts.recorded.map(\.messageText)
    AutomationAlerts.reset()
    try require(said == [MagicEraser.targetRefusal] && document.doc === masked,
                "\(tag) a mask target refuses by name: \(said)")
    controller.setPaintTarget(.layer)
}
