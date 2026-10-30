import AppKit

/// Captures actual canvas drawing, including the committed and proposed
/// marquees. The cursor itself is captured from the NSCursor image because
/// AppKit deliberately excludes the pointer from view snapshots.
private func selectionFrame(_ canvas: ImageCanvasView) throws -> NSBitmapImageRep {
    guard let bitmap = canvas.bitmapImageRepForCachingDisplay(in: canvas.bounds) else {
        throw DoctoredCoreError(message: "Selection canvas cache unavailable")
    }
    canvas.cacheDisplay(in: canvas.bounds, to: bitmap)
    return bitmap
}

private func selectionRegion(_ bitmap: NSBitmapImageRep, rightEdge: Int) throws -> [Int] {
    var values: [Int] = []
    for y in 0..<min(bitmap.pixelsHigh, 32) {
        for x in 0..<min(bitmap.pixelsWide, rightEdge) {
            guard let color = bitmap.colorAt(x: x, y: y)?.usingColorSpace(.deviceRGB) else {
                throw DoctoredCoreError(message: "Selection canvas pixel unavailable")
            }
            values += [color.redComponent, color.greenComponent,
                       color.blueComponent, color.alphaComponent].map {
                Int(($0 * 255).rounded())
            }
        }
    }
    return values
}

private func selectionCursorData(_ cursor: NSCursor) throws -> Data {
    guard let data = cursor.image.tiffRepresentation, !data.isEmpty else {
        throw DoctoredCoreError(message: "Selection cursor image unavailable")
    }
    return data
}

private func selectionMouseMoved(_ canvas: ImageCanvasView,
                                 modifiers: NSEvent.ModifierFlags) throws {
    guard let window = canvas.window,
          let event = NSEvent.mouseEvent(
            with: .mouseMoved, location: canvas.convert(CGPoint(x: 48, y: 32), to: nil),
            modifierFlags: modifiers, timestamp: ProcessInfo.processInfo.systemUptime,
            windowNumber: window.windowNumber, context: nil, eventNumber: 0,
            clickCount: 0, pressure: 0) else {
        throw DoctoredCoreError(message: "Selection hover event unavailable")
    }
    canvas.mouseMoved(with: event)
}

func selectionModifierFeedbackUI(output: URL) throws -> [String: Any] {
    let (document, editor, source) = try openDocument()
    defer { document.close(); pump() }
    let canvas = editor.canvas
    editor.view.layoutSubtreeIfNeeded()
    let oldRect = CGRect(x: 8, y: 8, width: 18, height: 14)
    guard let original = CanvasSelection(shape: .rect(oldRect),
        canvasWidth: source.width, canvasHeight: source.height) else {
        throw DoctoredCoreError(message: "Original selection unavailable")
    }

    var cursorHashes: [String: [String: String]] = [:]
    let tools: [EditorTool] = [.select, .ellipseSelect, .lasso,
                               .freehandLasso, .quickSelect, .wand, .subject]
    for tool in tools {
        canvas.tool = tool
        try selectionMouseMoved(canvas, modifiers: [])
        let normal = canvas.selectionFeedbackCursor(latchedMode: nil)
        try selectionMouseMoved(canvas, modifiers: [.shift])
        let addCursor = canvas.selectionFeedbackCursor(latchedMode: nil)
        let add = try selectionCursorData(addCursor)
        try selectionMouseMoved(canvas, modifiers: [.option])
        let subtractCursor = canvas.selectionFeedbackCursor(latchedMode: nil)
        let subtract = try selectionCursorData(subtractCursor)
        try require(normal !== addCursor && normal !== subtractCursor && add != subtract,
            "\(tool.displayName) did not distinguish replace, + and - cursors")
        cursorHashes[tool.displayName] = ["add": hashBytes(Array(add)),
            "subtract": hashBytes(Array(subtract))]
    }
    editor.selectTool(.select)
    try selectionMouseMoved(canvas, modifiers: [])
    guard let window = canvas.window,
          let optionsField = descendants(editor.view)
            .compactMap({ $0 as? OptionFieldControl })
            .flatMap(descendants)
            .compactMap({ $0 as? NSTextField })
            .first(where: { $0.isEditable && !$0.isHiddenOrHasHiddenAncestor }),
          window.makeFirstResponder(optionsField) else {
        throw DoctoredCoreError(message: "Selection options field could not take focus")
    }
    try require(window.firstResponder !== canvas && optionsField.currentEditor() != nil,
        "Selection options field was not the actual first responder")
    try require(canvas.selectionCursorHovered,
        "Selection cursor was not hovering before the modifier event")
    let replaceCursor = canvas.selectionFeedbackCursor(latchedMode: nil)
    func sendFlags(_ modifiers: NSEvent.ModifierFlags, keyCode: UInt16) throws {
        guard let changed = NSEvent.keyEvent(
            with: .flagsChanged, location: canvas.convert(CGPoint(x: 48, y: 32), to: nil),
            modifierFlags: modifiers, timestamp: ProcessInfo.processInfo.systemUptime,
            windowNumber: window.windowNumber, context: nil,
            characters: "", charactersIgnoringModifiers: "", isARepeat: false,
            keyCode: keyCode) else {
            throw DoctoredCoreError(message: "Selection modifier event unavailable")
        }
        NSApp.sendEvent(changed)
        try require(window.firstResponder !== canvas && optionsField.currentEditor() != nil,
            "Selection options field lost focus during modifier routing")
    }
    // Exercise the real application event route while the options field has
    // focus and the pointer stays over the canvas.
    try sendFlags([.shift], keyCode: 56)
    let shiftedCursor = canvas.selectionFeedbackCursor(latchedMode: nil)
    let shiftedData = try selectionCursorData(shiftedCursor)
    try require(canvas.selectionCursorModifiers.contains(.shift) &&
                shiftedCursor !== replaceCursor &&
                hashBytes(Array(shiftedData)) == cursorHashes[EditorTool.select.displayName]?["add"],
        "Stationary Shift did not display the add selection cursor")
    try sendFlags([.option], keyCode: 58)
    let optionCursor = canvas.selectionFeedbackCursor(latchedMode: nil)
    let optionData = try selectionCursorData(optionCursor)
    try require(canvas.selectionCursorModifiers.contains(.option) &&
                optionCursor !== shiftedCursor &&
                optionData != shiftedData &&
                hashBytes(Array(optionData)) == cursorHashes[EditorTool.select.displayName]?["subtract"],
        "Stationary Option did not display the subtract selection cursor")
    try sendFlags([], keyCode: 58)
    try require(canvas.selectionFeedbackCursor(latchedMode: nil) === replaceCursor,
        "Releasing modifiers did not restore the replace cursor")

    var drags: [[String: Any]] = []
    for (tool, mode, modifiers, destination) in [
        (EditorTool.select, "add", NSEvent.ModifierFlags.shift,
         CGPoint(x: 56, y: 36)),
        (.select, "subtract", .option, CGPoint(x: 23, y: 21)),
        (.ellipseSelect, "replace", [], CGPoint(x: 56, y: 36)),
    ] {
        editor.agentSetSelection(original)
        editor.selectTool(tool)
        let start = mode == "subtract" ? CGPoint(x: 14, y: 11)
            : CGPoint(x: 36, y: 20)
        let beforeSelection = canvas.selection?.maskBytes()
        let beforeAnts = canvas.marchingAntsView?.shapeLayer?.path
        let beforeFrame = try selectionFrame(canvas)
        let beforeRegion = try selectionRegion(beforeFrame,
            rightEdge: Int(Double(beforeFrame.pixelsWide) * 30 / Double(source.width)))
        let beforeImage = document.doc
        let beforeHistory = document.history.entries.count
        let beforeDirty = document.isDocumentEdited
        _ = try send(canvas, .leftMouseDown, start, modifiers: modifiers)
        _ = try send(canvas, .leftMouseDragged, destination, modifiers: modifiers)
        try require(canvas.marqueePreview != nil,
            "\(tool.displayName) did not make a live proposed outline")
        try require(canvas.selection?.maskBytes() == beforeSelection,
            "\(tool.displayName) hid or replaced the original selection during drag")
        try require(canvas.marchingAntsView?.isHidden == false && beforeAnts != nil
                    && canvas.marchingAntsView?.shapeLayer?.path == beforeAnts,
            "\(tool.displayName) hid or changed the original selection's ants during drag")
        try require(document.doc === beforeImage &&
                    document.history.entries.count == beforeHistory &&
                    document.isDocumentEdited == beforeDirty,
            "\(tool.displayName) drag changed document/history/dirty state")
        let duringFrame = try selectionFrame(canvas)
        let duringRegion = try selectionRegion(duringFrame,
            rightEdge: Int(Double(duringFrame.pixelsWide) * 30 / Double(source.width)))
        if mode != "subtract" {
            try require(beforeRegion == duringRegion,
                "\(tool.displayName) drag changed the original marquee's visible pixels")
        }
        try require(beforeFrame.tiffRepresentation != duringFrame.tiffRepresentation,
            "\(tool.displayName) proposed outline did not appear on canvas")
        _ = try send(canvas, .leftMouseUp, destination, modifiers: modifiers)
        try require(canvas.marqueePreview == nil,
            "\(tool.displayName) left a proposed outline after release")
        guard let committed = canvas.selection else {
            throw DoctoredCoreError(message: "\(tool.displayName) did not commit selection")
        }
        let samples = committed.maskBytes()
        func covered(_ x: Int, _ y: Int) -> Bool {
            samples[y * source.width + x] > 0
        }
        switch mode {
        case "add":
            try require(covered(10, 10) && covered(45, 25),
                "Shift drag did not combine original and new selection")
        case "subtract":
            try require(covered(10, 10) && !covered(18, 16),
                "Option drag did not subtract from original selection")
        default:
            try require(!covered(10, 10) && covered(45, 25),
                "Replace drag did not replace original selection on release")
        }
        drags.append(["tool": tool.displayName, "mode": mode,
            "original_visible_during_drag": true,
            "proposed_outline_visible": true, "commit_correct": true])
    }
    return ["case": "selection-modifier-feedback-ui", "cursor_hashes": cursorHashes,
        "stationary_shift_option_with_options_focus": true, "drags": drags]
}
