import AppKit

/// Presses the bare letter `character` through the current layout's key code.
private func toolKeysPress(_ character: String) throws {
    guard let code = canvasKeysCode(for: character) else {
        throw DoctoredCoreError(message: "No key on the current layout types \(character)")
    }
    try canvasKeysPress(character.uppercased(), code: code, expect: character)
}

/// Bare tool letters are the shipped default table
/// (`designs/v1-shortcuts-fade.md`), a shared key cycles its tools in rail order and first returns to the one last used, X swaps and D resets the shared
/// foreground/background pair (white/black on a layer-mask target), and the rail's two glyphs
/// do the same. A text field keeps X and D, and none of it records history.
func toolKeysUI() throws -> [String: Any] {
    let previousPolicy = NSApp.activationPolicy()
    let previousMenu = NSApp.mainMenu
    let previousShared = ToolOptionsStore.shared.sharedState
    _ = NSApp.setActivationPolicy(.regular)
    let delegate = AppDelegate()
    NSApp.mainMenu = delegate.makeMainMenu()
    defer {
        ToolOptionsStore.shared.sharedState = previousShared
        NSApp.mainMenu = previousMenu
        _ = NSApp.setActivationPolicy(previousPolicy)
        withExtendedLifetime(delegate) {}
    }

    let (document, editor, _) = try openBytePaintDocument(opaque: true)
    defer { document.close(); pump() }
    let canvas = editor.canvas
    guard let window = canvas.window else {
        throw DoctoredCoreError(message: "The owned editor has no window")
    }
    window.makeKeyAndOrderFront(nil)
    NSApp.activate(ignoringOtherApps: true)
    try waitUntil({ NSApp.keyWindow === window && window.isKeyWindow },
        "The owned editor never became the real key window")
    try require(window.makeFirstResponder(canvas), "The canvas refused first responder")
    try require(window.firstResponder === canvas, "The canvas is not the first responder")
    var reported: [String: Any] = [:]

    func walk(_ key: String, _ expected: [EditorTool]) throws {
        var landed: [EditorTool] = []
        for _ in expected {
            try toolKeysPress(key)
            landed.append(editor.currentTool)
        }
        try require(landed == expected, "\(key.uppercased()) walked \(landed), not \(expected)")
        reported["walk_\(key)"] = landed.map(\.identifier)
    }

    // (a) The default letters on a fresh editor.
    let letters: [(String, EditorTool)] = [
        ("s", .clone), ("o", .dodge), ("j", .spotHeal), ("u", .shapeRect), ("w", .wand),
        ("m", .select), ("l", .lasso),
    ]
    for (key, tool) in letters {
        try toolKeysPress(key)
        try require(editor.currentTool == tool,
            "\(key.uppercased()) selected \(editor.currentTool), not \(tool)")
    }

    // (b) Each run's first press returns to the remembered member, then cycles in rail order.
    try walk("j", [.spotHeal, .heal, .patch, .redEye, .spotHeal])
    try walk("w", [.wand, .quickSelect, .subject, .wand])
    try walk("u", [.shapeRect, .shapeEllipse, .shapeLine, .shapeRect])
    try walk("m", [.select, .ellipseSelect, .select])
    // A peer the document can't use is skipped.
    try require(EditorTool.tool(forKey: "j", current: .spotHeal, last: nil,
                                available: { $0 != .heal }) == .patch,
                "J did not skip an unavailable Heal")

    // (c) A rail/menu pick counts as the key's memory.
    editor.selectTool(.heal); pump()
    try toolKeysPress("b")
    try require(editor.currentTool == .brush, "B selected \(editor.currentTool)")
    try toolKeysPress("j")
    try require(editor.currentTool == .heal, "J after a Heal pick landed on \(editor.currentTool)")

    // (d) P is the Pen's and R the focus tools'.
    try toolKeysPress("p")
    try require(editor.currentTool == .pen, "P selected \(editor.currentTool), not the Pen")
    try toolKeysPress("r")
    try require(editor.currentTool == .blurBrush, "R selected \(editor.currentTool), not Blur")

    func pairHexes() -> (String, String) {
        (TextLayer.hex(editor.paintColor), TextLayer.hex(editor.backgroundColor))
    }
    func requirePair(_ fg: String, _ bg: String, _ label: String) throws {
        let (paint, background) = pairHexes()
        try require(paint == fg && background == bg,
            "\(label): paint \(paint) / background \(background), not \(fg) / \(bg)")
        let shared = ToolOptionsStore.shared.sharedState
        try require(shared.foreground == fg && shared.background == bg,
            "\(label): shared \(shared.foreground) / \(shared.background), not \(fg) / \(bg)")
        try require(TextLayer.hex(editor.toolRail.foregroundSwatchColor) == fg
                    && TextLayer.hex(editor.toolRail.backgroundSwatchColor) == bg,
            "\(label): the rail swatches do not show \(fg) / \(bg)")
    }
    /// Runs `body` and requires the history entry count unchanged across it alone.
    func noHistory(_ label: String, _ body: () throws -> Void) throws {
        let before = document.history.entries.count
        try body()
        try require(document.history.entries.count == before, "\(label) recorded history")
    }

    // (e) X swaps and D resets; neither moves the tool.
    let red = "#FF0000FF", blue = "#0000FFFF", black = "#000000FF", white = "#FFFFFFFF"
    editor.setPaintColor(NSColor(srgbRed: 1, green: 0, blue: 0, alpha: 1))
    editor.setBackgroundColor(NSColor(srgbRed: 0, green: 0, blue: 1, alpha: 1))
    try requirePair(red, blue, "Setup")
    let toolBeforePair = editor.currentTool
    try noHistory("X") { try toolKeysPress("x") }
    try requirePair(blue, red, "X")
    try noHistory("D") { try toolKeysPress("d") }
    try requirePair(black, white, "D")
    try require(editor.currentTool == toolBeforePair, "D changed the tool to \(editor.currentTool)")

    // (f) A layer-mask target resets white/black; Quick Mask keeps black/white.
    _ = try call("add_layer_mask", ["layer": 0, "kind": "reveal_all"], document: document)
    pump()
    editor.selectTool(.brush); pump()
    editor.setPaintTarget(.mask); pump()
    try require(editor.paintTarget == .mask, "The mask is not the paint target")
    try noHistory("D on a mask target") { try toolKeysPress("d") }
    try requirePair(white, black, "D on a mask target")
    editor.toggleQuickMask(nil)
    try waitUntil({ canvas.quickMaskActive }, "Quick Mask never turned on")
    try noHistory("D in Quick Mask") { try toolKeysPress("d") }
    try requirePair(black, white, "D in Quick Mask")
    editor.toggleQuickMask(nil)
    try waitUntil({ !canvas.quickMaskActive }, "Quick Mask never turned off")
    editor.setPaintTarget(.layer); pump()

    // (g) The rail's glyphs call the same two methods.
    editor.setPaintColor(NSColor(srgbRed: 1, green: 0, blue: 0, alpha: 1))
    editor.setBackgroundColor(NSColor(srgbRed: 0, green: 0, blue: 1, alpha: 1))
    try noHistory("The swap glyph") { editor.toolRail.onSwapColors?(); pump() }
    try requirePair(blue, red, "The swap glyph")
    try noHistory("The default-colours glyph") { editor.toolRail.onResetColors?(); pump() }
    try requirePair(black, white, "The default-colours glyph")

    // (h) A text field keeps X and D.
    editor.setPaintColor(NSColor(srgbRed: 1, green: 0, blue: 0, alpha: 1))
    editor.setBackgroundColor(NSColor(srgbRed: 0, green: 0, blue: 1, alpha: 1))
    let field = NSTextField(frame: CGRect(x: 0, y: 0, width: 120, height: 22))
    editor.view.addSubview(field)
    defer { field.removeFromSuperview() }
    try require(window.makeFirstResponder(field), "The text field refused first responder")
    let toolBeforeField = editor.currentTool
    try noHistory("X and D in a text field") {
        try toolKeysPress("x")
        try toolKeysPress("d")
    }
    try requirePair(red, blue, "X and D in a text field")
    try require(editor.currentTool == toolBeforeField,
        "X or D in a text field changed the tool to \(editor.currentTool)")
    field.removeFromSuperview()
    _ = window.makeFirstResponder(canvas)
    reported["color_pair"] = true

    return ["case": "tool-keys-ui", "passed": true, "results": reported]
}
