import AppKit

/// One synthetic hover at `point` through the canvas's actual mouseMoved handler.
private func outlineMouseMoved(_ canvas: ImageCanvasView, _ point: CGPoint,
                               modifiers: NSEvent.ModifierFlags = []) throws {
    guard let window = canvas.window,
          let event = NSEvent.mouseEvent(
            with: .mouseMoved, location: canvas.convert(point, to: nil),
            modifierFlags: modifiers, timestamp: ProcessInfo.processInfo.systemUptime,
            windowNumber: window.windowNumber, context: nil, eventNumber: 0,
            clickCount: 0, pressure: 0) else {
        throw DoctoredCoreError(message: "Brush outline hover event unavailable")
    }
    canvas.mouseMoved(with: event)
    pump()
}

private func outlineMouseExited(_ canvas: ImageCanvasView) throws {
    guard let window = canvas.window,
          let event = NSEvent.enterExitEvent(
            with: .mouseExited, location: canvas.convert(CGPoint(x: -10, y: -10), to: nil),
            modifierFlags: [], timestamp: ProcessInfo.processInfo.systemUptime,
            windowNumber: window.windowNumber, context: nil, eventNumber: 0,
            trackingNumber: 0, userData: nil) else {
        throw DoctoredCoreError(message: "Brush outline exit event unavailable")
    }
    canvas.mouseExited(with: event)
    pump()
}

/// The canvas's actual drawing (AppKit excludes the pointer from view snapshots).
private func outlineFrame(_ canvas: ImageCanvasView) throws -> NSBitmapImageRep {
    guard let bitmap = canvas.bitmapImageRepForCachingDisplay(in: canvas.bounds) else {
        throw DoctoredCoreError(message: "Brush outline canvas cache unavailable")
    }
    canvas.cacheDisplay(in: canvas.bounds, to: bitmap)
    return bitmap
}

private func outlineFrameBytes(_ bitmap: NSBitmapImageRep) throws -> [UInt8] {
    guard let data = bitmap.bitmapData else {
        throw DoctoredCoreError(message: "Brush outline frame bytes unavailable")
    }
    return Array(UnsafeBufferPointer(start: data, count: bitmap.bytesPerRow * bitmap.pixelsHigh))
}

/// The 3×3 bitmap window around canvas point (x, y), mapped canvas px → bitmap px.
private func outlineWindow(_ bitmap: NSBitmapImageRep, x: CGFloat, y: CGFloat,
                           canvasWidth: Int) throws -> [Int] {
    let scale = CGFloat(bitmap.pixelsWide) / CGFloat(canvasWidth)
    let cx = Int((x * scale).rounded(.down)), cy = Int((y * scale).rounded(.down))
    var values: [Int] = []
    for py in (cy - 1)...(cy + 1) {
        for px in (cx - 1)...(cx + 1) {
            guard px >= 0, py >= 0, px < bitmap.pixelsWide, py < bitmap.pixelsHigh,
                  let color = bitmap.colorAt(x: px, y: py)?.usingColorSpace(.deviceRGB) else {
                throw DoctoredCoreError(message: "Brush outline pixel (\(px), \(py)) unavailable")
            }
            values += [color.redComponent, color.greenComponent,
                       color.blueComponent, color.alphaComponent].map {
                Int(($0 * 255).rounded())
            }
        }
    }
    return values
}

/// The stroke tools draw the tip's full footprint under the pointer
/// and hide the system pointer; the crosshair returns below 6 screen points
/// and for ⌥ on the tools whose ⌥-press samples one pixel.
func brushOutlineCursorUI() throws -> [String: Any] {
    let (document, editor, source) = try openBytePaintDocument(opaque: true)
    defer { document.close(); pump() }
    let canvas = editor.canvas
    let width = source.width
    let hidden = ImageCanvasView.brushOutlineHiddenCursor
    func windowAt(_ frame: NSBitmapImageRep, _ x: CGFloat, _ y: CGFloat) throws -> [Int] {
        try outlineWindow(frame, x: x, y: y, canvasWidth: width)
    }

    // a. Baseline frame, no hover.
    configure(editor, size: 24, hardness: 1, opacity: 100, flow: 100)
    editor.scrollView.magnification = 1; pump()
    try outlineMouseExited(canvas)
    let f0 = try outlineFrame(canvas)
    let f0Bytes = try outlineFrameBytes(f0)

    // b. Hover draws the ring and hides the pointer; the document is untouched.
    let doc0 = document.doc
    let history0 = document.history.entries.count
    let edited0 = document.isDocumentEdited
    try outlineMouseMoved(canvas, CGPoint(x: 48, y: 32))
    try require(canvas.hoverPoint == CGPoint(x: 48, y: 32),
        "Hover did not latch (48,32): \(String(describing: canvas.hoverPoint))")
    try require(canvas.brushOutlineCursor === hidden, "Brush hover did not hide the pointer")
    let f1 = try outlineFrame(canvas)
    for (x, y) in [(60.0, 32.0), (36.0, 32.0), (48.0, 20.0)] {
        try require(try windowAt(f1, x, y) != windowAt(f0, x, y),
            "Brush outline missing at (\(x), \(y))")
    }
    try require(try windowAt(f1, 48, 32) == windowAt(f0, 48, 32),
        "Brush outline drew over its own centre")
    try require(document.doc === doc0, "Hover replaced the document")
    try require(document.history.entries.count == history0, "Hover added a history entry")
    try require(document.isDocumentEdited == edited0, "Hover dirtied the document")

    // c. Size gate: under 6 screen points the crosshair returns and nothing is drawn.
    ToolOptionsStore.shared.brush.size = 4
    editor.syncCanvasPaintState(); pump()
    try require(canvas.brushOutlineCursor == nil, "4 px brush at 100% kept the hidden pointer")
    try require(try outlineFrameBytes(outlineFrame(canvas)) == f0Bytes,
        "4 px brush at 100% still drew an outline")
    editor.scrollView.magnification = 4; pump()
    try require(canvas.brushOutlineCursor === hidden,
        "4 px brush at 400% did not hide the pointer")
    try outlineMouseExited(canvas)
    let g0 = try outlineFrame(canvas)
    try outlineMouseMoved(canvas, CGPoint(x: 48, y: 32))
    let g1 = try outlineFrame(canvas)
    try require(try windowAt(g1, 50, 32) != windowAt(g0, 50, 32),
        "4 px brush outline missing at 400%")

    // d. Viewport refresh re-derives the hover point from a window location.
    try outlineMouseMoved(canvas, CGPoint(x: 24, y: 16))
    try require(canvas.hoverPoint == CGPoint(x: 24, y: 16),
        "Hover did not latch (24,16) at 400%")
    let w = canvas.convert(CGPoint(x: 24, y: 16), to: nil)
    editor.scrollView.magnification = 1; pump()
    canvas.refreshHoverPoint(windowLocation: w)
    let local = canvas.convert(w, from: nil)
    let expected = CGPoint(x: min(max(local.x, 0), canvas.bounds.width),
                           y: min(max(local.y, 0), canvas.bounds.height))
    try require(canvas.hoverPoint == expected,
        "Viewport refresh gave \(String(describing: canvas.hoverPoint)), expected \(expected)")
    try require(canvas.hoverPoint != CGPoint(x: 24, y: 16),
        "Viewport refresh left the stale hover point")
    try outlineMouseExited(canvas)
    canvas.refreshHoverPoint(windowLocation: w)
    try require(canvas.hoverPoint == nil, "Viewport refresh latched a point while not hovered")

    // e. Tip angle and roundness shape the ring exactly as the dab.
    ToolOptionsStore.shared.brush.size = 24
    editor.syncCanvasPaintState(); pump()
    try outlineMouseMoved(canvas, CGPoint(x: 48, y: 32))
    ToolOptionsStore.shared.brush.angle = 45
    ToolOptionsStore.shared.brush.roundness = 50
    editor.syncCanvasPaintState(); pump()
    let e1 = try outlineFrame(canvas)
    try require(try windowAt(e1, 56, 24) != windowAt(f0, 56, 24),
        "45° / 50% outline missing along its long axis")
    try require(try windowAt(e1, 56, 40) == windowAt(f0, 56, 40),
        "45° / 50% outline drew across its short axis")
    try require(try windowAt(e1, 48, 20) == windowAt(f0, 48, 20),
        "45° / 50% outline drew where the round ring was")

    // f. Leaving the canvas clears the ring.
    ToolOptionsStore.shared.brush.angle = 0
    ToolOptionsStore.shared.brush.roundness = 100
    editor.syncCanvasPaintState(); pump()
    try outlineMouseExited(canvas)
    try require(canvas.hoverPoint == nil, "Exit left a hover point")
    try require(try outlineFrameBytes(outlineFrame(canvas)) == f0Bytes,
        "Exit left the outline drawn")

    // g. The outline follows a stroke.
    _ = try send(canvas, .leftMouseDown, CGPoint(x: 20, y: 20))
    _ = try send(canvas, .leftMouseDragged, CGPoint(x: 60, y: 40))
    try require(canvas.hoverPoint == CGPoint(x: 60, y: 40),
        "Drag did not move the hover point: \(String(describing: canvas.hoverPoint))")
    _ = try send(canvas, .leftMouseUp, CGPoint(x: 60, y: 40))
    pump()

    // h. ⌥ restores the crosshair only where ⌥ samples one pixel.
    try outlineMouseMoved(canvas, CGPoint(x: 48, y: 32), modifiers: [.option])
    try require(canvas.brushOutlineCursor == nil, "⌥ on the brush kept the hidden pointer")
    try require(canvas.brushOutline != nil, "⌥ on the brush dropped the ring")
    try outlineMouseMoved(canvas, CGPoint(x: 48, y: 32))
    try require(canvas.brushOutlineCursor === hidden, "Releasing ⌥ did not hide the pointer")
    var tip = PaintToolOptions()
    tip.size = 24
    ToolOptionsStore.shared.clone = tip
    ToolOptionsStore.shared.eraser = tip
    editor.selectTool(.clone); editor.syncCanvasPaintState(); pump()
    try outlineMouseMoved(canvas, CGPoint(x: 48, y: 32), modifiers: [.option])
    try require(canvas.brushOutlineCursor == nil, "⌥ on the clone stamp kept the hidden pointer")
    editor.selectTool(.eraser); editor.syncCanvasPaintState(); pump()
    try outlineMouseMoved(canvas, CGPoint(x: 48, y: 32), modifiers: [.option])
    try require(canvas.brushOutlineCursor === hidden, "⌥ on the eraser showed the crosshair")

    // i. Tool switches re-derive the cursor.
    editor.selectTool(.eraser); pump()
    try outlineMouseMoved(canvas, CGPoint(x: 48, y: 32))
    try require(canvas.brushOutlineCursor === hidden, "Eraser hover did not hide the pointer")
    editor.selectTool(.move); pump()
    try require(canvas.brushOutlineCursor == nil, "Move tool kept the hidden pointer")

    return ["case": "brush-outline-cursor-ui", "passed": true,
            "hover_frame": hashBytes(try outlineFrameBytes(f1)),
            "tip_frame": hashBytes(try outlineFrameBytes(e1))]
}
