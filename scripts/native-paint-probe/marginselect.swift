import AppKit

/// The window point of a canvas-space point, required to land on the well's void (outside the
/// canvas host), so the press provably reaches the clip view rather than the canvas.
private func voidWindowPoint(_ editor: EditorViewController, _ point: CGPoint) throws -> CGPoint {
    let clip = editor.scrollView.contentView
    let window = editor.canvas.convert(point, to: nil)
    try require(!editor.canvasHost.frame.contains(clip.convert(window, from: nil)),
        "Margin press at \(point) is not on the void")
    return window
}

/// A drag that begins on the void, every event sent to the CLIP VIEW — AppKit's mouse-down-view
/// routing for a press outside the canvas.
private func marginDrag(_ editor: EditorViewController, from: CGPoint, via: [CGPoint] = [],
                        to: CGPoint, modifiers: NSEvent.ModifierFlags = []) throws {
    let clip = editor.scrollView.contentView
    let down = try voidWindowPoint(editor, from)
    try sendWindow(clip, .leftMouseDown, windowPoint: down)
    for point in via + [to] {
        try sendWindow(clip, .leftMouseDragged,
            windowPoint: editor.canvas.convert(point, to: nil), modifiers: modifiers)
    }
    try sendWindow(clip, .leftMouseUp, windowPoint: editor.canvas.convert(to, to: nil),
        modifiers: modifiers)
    pump()
}

/// A press and release on the void with no drag tick.
private func marginClick(_ editor: EditorViewController, at point: CGPoint) throws {
    let clip = editor.scrollView.contentView
    let window = try voidWindowPoint(editor, point)
    try sendWindow(clip, .leftMouseDown, windowPoint: window)
    try sendWindow(clip, .leftMouseUp, windowPoint: window)
    pump()
}

private func boundsArray(_ rect: CGRect) -> [Double] {
    [Double(rect.minX), Double(rect.minY), Double(rect.width), Double(rect.height)]
}

/// A marquee, freehand-lasso or Quick Selection drag may begin on the pasteboard and is
/// clipped to the image; a click there stays inert for every tool, and the gate refuses the rest.
func marginSelectStartUI() throws -> [String: Any] {
    let (document, editor, _) = try openBytePaintDocument(opaque: true)
    defer { document.close(); pump() }
    let canvas = editor.canvas
    try editor.applyViewState(zoom: 1, center: CGPoint(x: 48, y: 32))
    pump()
    editor.view.layoutSubtreeIfNeeded()
    var reported: [String: Any] = [:]

    // a. Rectangle marquee from above the image.
    editor.selectTool(.select); pump()
    var history = document.history.entries.count
    try marginDrag(editor, from: CGPoint(x: 30, y: -40), to: CGPoint(x: 70, y: 40),
        modifiers: [.control])
    let a = canvas.selection?.bounds
    try require(a == CGRect(x: 30, y: 0, width: 40, height: 40),
        "Marquee from above did not start at the top edge: \(String(describing: a))")
    try require(document.history.entries.count == history + 1,
        "Marquee from the void was not one history step")
    reported["a"] = a.map(boundsArray)

    // b. Ellipse marquee from the left.
    editor.selectTool(.ellipseSelect); pump()
    try marginDrag(editor, from: CGPoint(x: -25, y: 20), to: CGPoint(x: 50, y: 60),
        modifiers: [.control])
    let b = canvas.selection?.bounds
    try require(b == CGRect(x: 0, y: 20, width: 50, height: 40),
        "Ellipse from the left did not start at the left edge: \(String(describing: b))")
    reported["b"] = b.map(boundsArray)

    // c. Rectangle marquee from the bottom-right void.
    editor.selectTool(.select); pump()
    try marginDrag(editor, from: CGPoint(x: 120, y: 90), to: CGPoint(x: 60, y: 30),
        modifiers: [.control])
    let c = canvas.selection?.bounds
    try require(c == CGRect(x: 60, y: 30, width: 36, height: 34),
        "Marquee from the bottom-right did not start at the corner: \(String(describing: c))")
    reported["c"] = c.map(boundsArray)

    // f. Freehand lasso from the left.
    editor.selectTool(.freehandLasso); pump()
    history = document.history.entries.count
    try marginDrag(editor, from: CGPoint(x: -20, y: 10), via: [CGPoint(x: 40, y: 10)],
        to: CGPoint(x: 40, y: 50))
    let f = canvas.selection?.bounds
    try require(f == CGRect(x: 0, y: 10, width: 40, height: 40),
        "Freehand lasso from the left did not start at the left edge: \(String(describing: f))")
    try require(document.history.entries.count == history + 1,
        "Freehand lasso from the void was not one history step")
    reported["f"] = f.map(boundsArray)

    // h. Quick Selection from above.
    editor.selectTool(.quickSelect); pump()
    history = document.history.entries.count
    try marginDrag(editor, from: CGPoint(x: 30, y: -40), to: CGPoint(x: 30, y: 20))
    guard let h = canvas.selection?.bounds else {
        throw DoctoredCoreError(message: "Quick Selection from the void selected nothing")
    }
    try require(CGRect(x: 0, y: 0, width: 96, height: 64).contains(h),
        "Quick Selection from the void escaped the canvas: \(h)")
    try require(document.history.entries.count == history + 1,
        "Quick Selection from the void was not one history step")
    reported["h"] = boundsArray(h)

    // i. A marquee click on the void is dropped, not a deselect.
    editor.selectTool(.select); pump()
    history = document.history.entries.count
    try marginClick(editor, at: CGPoint(x: -30, y: -30))
    try require(canvas.selection?.bounds == h,
        "Marquee click on the void changed the selection: \(String(describing: canvas.selection?.bounds))")
    try require(document.history.entries.count == history,
        "Marquee click on the void recorded history")

    // g. A Quick Selection click on the void selects nothing and leaks no session.
    editor.selectTool(.quickSelect); pump()
    history = document.history.entries.count
    try marginClick(editor, at: CGPoint(x: 30, y: -40))
    try require(canvas.selection?.bounds == h,
        "Quick Selection click on the void changed the selection")
    try require(document.history.entries.count == history,
        "Quick Selection click on the void recorded history")
    try require(canvas.selectionStroke == nil,
        "Quick Selection click on the void leaked a selection stroke")

    // d. A marquee drag wholly on the void clamps to a degenerate rect: a deselect.
    editor.selectTool(.select); pump()
    try marginDrag(editor, from: CGPoint(x: -30, y: -30), to: CGPoint(x: -10, y: -10),
        modifiers: [.control])
    try require(canvas.selection == nil,
        "Marquee drag wholly on the void did not deselect: \(String(describing: canvas.selection?.bounds))")

    // e. Gate: the brush forwards nothing from the void.
    editor.selectTool(.brush); pump()
    history = document.history.entries.count
    try marginDrag(editor, from: CGPoint(x: -30, y: -30), to: CGPoint(x: -10, y: -10))
    try require(document.history.entries.count == history,
        "Brush drag from the void recorded history")
    try require(canvas.selection == nil, "Brush drag from the void changed the selection")

    return ["case": "margin-select-start-ui", "passed": true, "bounds": reported]
}
