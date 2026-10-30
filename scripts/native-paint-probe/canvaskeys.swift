import AppKit

/// A hardware-shaped press: the window server's own translation of `code` under `shift`, delivered
/// through `NSApp.sendEvent` so a bare key equivalent in the real main menu is resolved first,
/// exactly as a keyboard press is. The codes assume a US layout; a mismatch names the key.
func canvasKeysPress(_ name: String, code: CGKeyCode, shift: Bool = false,
                             expect: String) throws {
    guard let source = CGEventSource(stateID: .privateState),
          let cg = CGEvent(keyboardEventSource: source, virtualKey: code, keyDown: true)
    else {
        throw DoctoredCoreError(message: "Owned CGEvent for \(name) unavailable")
    }
    cg.flags = shift ? [.maskShift] : []
    guard let event = NSEvent(cgEvent: cg) else {
        throw DoctoredCoreError(message: "Owned key event for \(name) unavailable")
    }
    try require(event.charactersIgnoringModifiers == expect,
        "Key \(name) (code \(code)) translated to "
        + "\(String(describing: event.charactersIgnoringModifiers)), not \(expect) "
        + "(the probe assumes a US layout)")
    NSApp.sendEvent(event)
    pump()
}

/// The virtual key code the CURRENT layout translates to `character` unshifted (code 3 is F only
/// on a US layout; Colemak puts F on code 14). Nil when no key produces it.
func canvasKeysCode(for character: String) -> CGKeyCode? {
    guard let source = CGEventSource(stateID: .privateState) else { return nil }
    for code in CGKeyCode(0)..<CGKeyCode(128) {
        guard let cg = CGEvent(keyboardEventSource: source, virtualKey: code, keyDown: true)
        else { continue }
        cg.flags = []
        if NSEvent(cgEvent: cg)?.charactersIgnoringModifiers == character { return code }
    }
    return nil
}

private func canvasKeysDelete() throws { try canvasKeysPress("Delete", code: 51, expect: "\u{7F}") }

private func canvasKeysMenuItem(_ menu: NSMenu, _ action: Selector) throws -> NSMenuItem {
    func search(_ menu: NSMenu) -> NSMenuItem? {
        for item in menu.items {
            if item.action == action { return item }
            if let submenu = item.submenu, let found = search(submenu) { return found }
        }
        return nil
    }
    guard let item = search(menu) else {
        throw DoctoredCoreError(message: "The real menu has no \(NSStringFromSelector(action))")
    }
    return item
}

private func canvasKeysPixels(_ document: ImageDocument) throws -> [UInt8] {
    guard let pixels = document.doc?.layerPixels(0) else {
        throw DoctoredCoreError(message: "Layer 0 has no stored byte pixels")
    }
    return pixels
}

/// Bare ⌫ pops the Polygonal Lasso's last vertex and never reaches
/// Edit ▸ Clear while a selection gesture (a marquee, freehand lasso or Quick Selection drag
/// included) is pending; { } step hardness, , . < > step the built-in presets and / toggles
/// Lock Transparency. Tab hides or shows the tool rail, options bar and side panel together,
/// Shift+Tab the side panel alone and F toggles full screen, all only while the canvas has key
/// focus (View ▸ Hide/Show Panels is the menu path, with no key equivalent).
func canvasKeysUI() throws -> [String: Any] {
    let previousPolicy = NSApp.activationPolicy()
    let previousMenu = NSApp.mainMenu
    let previousBrush = ToolOptionsStore.shared.brush
    _ = NSApp.setActivationPolicy(.regular)
    let delegate = AppDelegate()
    let menu = delegate.makeMainMenu()
    NSApp.mainMenu = menu
    defer {
        ToolOptionsStore.shared.brush = previousBrush
        NSApp.mainMenu = previousMenu
        _ = NSApp.setActivationPolicy(previousPolicy)
        withExtendedLifetime(delegate) {}
    }
    let clearItem = try canvasKeysMenuItem(
        menu, #selector(EditorViewController.clearSelection(_:)))

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

    // (a) Four lasso clicks, ⌫ pops the fourth; Return commits the first three.
    editor.selectTool(.select); pump()
    _ = try call("select_rect", ["x": 4, "y": 4, "width": 16, "height": 16], document: document)
    pump()
    try require(canvas.selection != nil, "select_rect made no selection")
    try require(editor.validateUserInterfaceItem(clearItem),
        "Clear does not validate with a selection and nothing pending")
    editor.selectTool(.lasso); pump()
    let vertices = [CGPoint(x: 40, y: 8), CGPoint(x: 80, y: 8), CGPoint(x: 80, y: 48),
                    CGPoint(x: 30, y: 58)]
    for point in vertices {
        try send(canvas, .leftMouseDown, point, modifiers: [.control])
        try send(canvas, .leftMouseUp, point, modifiers: [.control])
        pump()
    }
    try require(canvas.hasPendingSelectionGesture, "Four lasso clicks left nothing pending")
    try require(!editor.validateUserInterfaceItem(clearItem),
        "Clear validates while a polygonal lasso is open")
    var pixels = try canvasKeysPixels(document)
    var history = document.history.entries.count
    try canvasKeysDelete()
    try require(try canvasKeysPixels(document) == pixels, "⌫ on an open lasso changed pixels")
    try require(document.history.entries.count == history, "⌫ on an open lasso recorded history")
    try require(canvas.hasPendingSelectionGesture, "One ⌫ discarded the whole polygon")
    try canvasKeysPress("Return", code: 36, expect: "\r")
    try require(!canvas.hasPendingSelectionGesture, "Return left the lasso open")
    guard let a = canvas.selection?.bounds else {
        throw DoctoredCoreError(message: "Return committed no polygon")
    }
    try require(a.minX >= 39 && a.maxY <= 49 && a.maxX >= 79 && a.minY <= 9,
        "The committed polygon does not exclude the popped vertex: \(a)")
    try require(document.history.entries.count == history + 1,
        "The lasso commit was not one history step")
    reported["a"] = [Double(a.minX), Double(a.minY), Double(a.width), Double(a.height)]

    // (b) Two clicks, ⌫ ⌫ unwinds the polygon without touching the selection.
    let selectionBefore = canvas.selection.map { $0.maskBytes() }
    pixels = try canvasKeysPixels(document)
    history = document.history.entries.count
    for point in vertices.prefix(2) {
        try send(canvas, .leftMouseDown, point, modifiers: [.control])
        try send(canvas, .leftMouseUp, point, modifiers: [.control])
        pump()
    }
    try canvasKeysDelete()
    try canvasKeysDelete()
    try require(!canvas.hasPendingSelectionGesture, "⌫ ⌫ left two lasso vertices pending")
    try require(canvas.selection.map { $0.maskBytes() } == selectionBefore,
        "Popping the lasso changed the selection")
    try require(try canvasKeysPixels(document) == pixels, "Popping the lasso changed pixels")
    try require(document.history.entries.count == history, "Popping the lasso recorded history")

    // (c) Positive control: with nothing pending the same press is Edit ▸ Clear.
    try require(editor.validateUserInterfaceItem(clearItem),
        "Clear does not validate once the lasso is gone")
    try canvasKeysDelete()
    try require(document.history.entries.count == history + 1, "⌫ did not fire Edit ▸ Clear")
    try require(try canvasKeysPixels(document) != pixels, "Edit ▸ Clear changed no pixels")

    // (d) { } walk the hardness ladder; the canvas mirrors it; size/opacity stay.
    var start = PaintToolOptions()
    start.size = 31; start.opacity = 64; start.hardness = 100
    ToolOptionsStore.shared.brush = start
    editor.selectTool(.brush); editor.syncCanvasPaintState(); pump()
    var hardness: [Double] = []
    for _ in 0..<5 {
        try canvasKeysPress("{", code: 33, shift: true, expect: "{")
        hardness.append(ToolOptionsStore.shared.brush.hardness)
    }
    try canvasKeysPress("}", code: 30, shift: true, expect: "}")
    hardness.append(ToolOptionsStore.shared.brush.hardness)
    try require(hardness == [75, 50, 25, 0, 0, 25], "The hardness ladder walked \(hardness)")
    try require(abs(Double(canvas.brushHardness) - 0.25) < 1e-9,
        "The canvas hardness is \(canvas.brushHardness), not 0.25")
    try require(ToolOptionsStore.shared.brush.size == 31
                && ToolOptionsStore.shared.brush.opacity == 64,
        "The hardness keys changed size or opacity")
    editor.selectTool(.select); pump()
    let brushBefore = ToolOptionsStore.shared.brush
    try canvasKeysPress("}", code: 30, shift: true, expect: "}")
    try require(ToolOptionsStore.shared.brush == brushBefore, "} under .select changed the brush")
    reported["hardness"] = hardness

    // (e) , . < > step the built-in presets from Custom, wrapping at both ends.
    var custom = PaintToolOptions()
    custom.size = 31; custom.opacity = 64; custom.hardness = 37
    ToolOptionsStore.shared.brush = custom
    editor.selectTool(.brush); editor.syncCanvasPaintState(); pump()
    try require(BrushPreset.matchIndex(of: ToolOptionsStore.shared.brush) == 0,
        "The preset walk does not start from Custom")
    let names = BrushPreset.popupItems
    let presses: [(String, CGKeyCode, Bool, String, String)] = [
        (".", 47, false, ".", "Hard Round"), (".", 47, false, ".", "Soft Round"),
        (",", 43, false, ",", "Hard Round"), (",", 43, false, ",", "Stippled"),
        (".", 47, false, ".", "Hard Round"), (".", 47, false, ".", "Soft Round"),
        (">", 47, true, ">", "Stippled"), ("<", 43, true, "<", "Hard Round"),
    ]
    var walked: [String] = []
    for (name, code, shift, expect, preset) in presses {
        try canvasKeysPress(name, code: code, shift: shift, expect: expect)
        let row = BrushPreset.matchIndex(of: ToolOptionsStore.shared.brush)
        walked.append(names[row])
        try require(names[row] == preset, "\(name) landed on \(names[row]), not \(preset)")
    }
    // The two Shift presses start where the plain step differs: Soft Round's . is Airbrush
    // and Stippled's , is Flat Shader.
    try require(ToolOptionsStore.shared.brush.size == 31
                && ToolOptionsStore.shared.brush.opacity == 64,
        "The preset keys changed size or opacity")
    reported["presets"] = walked

    // (g) A marquee, freehand lasso or Quick Selection drag in flight keeps ⌫ away
    // from Clear.
    for tool in [EditorTool.select, .ellipseSelect, .freehandLasso, .quickSelect] {
        editor.selectTool(.select); pump()
        _ = try call("select_rect", ["x": 4, "y": 4, "width": 16, "height": 16],
            document: document)
        pump()
        try require(canvas.selection != nil, "select_rect made no selection for \(tool)")
        editor.selectTool(tool); pump()
        try send(canvas, .leftMouseDown, CGPoint(x: 30, y: 20))
        try send(canvas, .leftMouseDragged, CGPoint(x: 60, y: 40))
        pump()
        try require(canvas.hasPendingSelectionGesture, "A \(tool) drag is not pending")
        try require(!editor.validateUserInterfaceItem(clearItem),
            "Clear validates during a \(tool) drag")
        pixels = try canvasKeysPixels(document)
        history = document.history.entries.count
        try canvasKeysDelete()
        try require(try canvasKeysPixels(document) == pixels,
            "⌫ during a \(tool) drag changed pixels")
        try require(document.history.entries.count == history,
            "⌫ during a \(tool) drag recorded history")
        try send(canvas, .leftMouseUp, CGPoint(x: 60, y: 40))
        try waitUntil({ !canvas.hasPendingSelectionGesture },
            "The \(tool) drag stayed pending after mouse-up")
    }

    // (f) / toggles Lock Transparency as one undo step; a text field keeps the key.
    try require(window.makeFirstResponder(canvas), "The canvas refused first responder")
    try require(document.doc?.lockFlags(0).contains(.transparency) == false,
        "Layer 0 starts with Lock Transparency")
    history = document.history.entries.count
    try canvasKeysPress("/", code: 44, expect: "/")
    try require(document.doc?.lockFlags(0).contains(.transparency) == true,
        "/ did not set Lock Transparency")
    try require(document.history.entries.count == history + 1, "/ was not one history step")
    try canvasKeysPress("/", code: 44, expect: "/")
    try require(document.doc?.lockFlags(0).contains(.transparency) == false,
        "A second / did not clear Lock Transparency")
    document.undoManager?.undo()
    pump()
    try require(document.doc?.lockFlags(0).contains(.transparency) == true,
        "Undo did not restore Lock Transparency")
    let field = NSTextField(frame: CGRect(x: 0, y: 0, width: 120, height: 22))
    editor.view.addSubview(field)
    defer { field.removeFromSuperview() }
    try require(window.makeFirstResponder(field), "The text field refused first responder")
    let locks = document.doc?.lockFlags(0)
    let cursor = document.history.cursor
    history = document.history.entries.count
    try canvasKeysPress("/", code: 44, expect: "/")
    try require(document.doc?.lockFlags(0) == locks, "/ in a text field changed a lock")
    try require(document.history.entries.count == history && document.history.cursor == cursor,
        "/ in a text field recorded history")
    field.removeFromSuperview()
    _ = window.makeFirstResponder(canvas)

    // (h) Tab / Shift+Tab / F on the canvas, View ▸ Hide/Show Panels, and text fields keep them.
    try require(window.makeFirstResponder(canvas), "The canvas refused first responder")
    let screenHistory = document.history.entries.count
    let screenCursor = document.history.cursor
    let panelsItem = try canvasKeysMenuItem(menu, #selector(EditorViewController.togglePanels(_:)))
    editor.view.layoutSubtreeIfNeeded()
    let wellFrame = editor.scrollView.frame
    func allShown() -> Bool {
        editor.toolChromeVisible && editor.layersPanelVisible
            && !editor.optionsBar.isHidden && !editor.toolRail.isHidden
    }
    func allHidden() -> Bool {
        !editor.toolChromeVisible && !editor.layersPanelVisible
            && editor.optionsBar.isHidden && editor.toolRail.isHidden
    }
    try require(allShown(), "The chrome does not start fully visible")
    try canvasKeysPress("Tab", code: 48, expect: "\t")
    try require(allHidden(), "Tab did not hide the rail, the options bar and the side panel")
    try require(window.firstResponder === canvas, "Tab moved key focus off the canvas")
    editor.view.layoutSubtreeIfNeeded()
    let hiddenFrame = editor.scrollView.frame
    try require(hiddenFrame.minX == wellFrame.minX - DS.railWidth,
        "Tab moved the well's left edge from \(wellFrame.minX) to \(hiddenFrame.minX)")
    try require(hiddenFrame.height == wellFrame.height + DS.optionsBarHeight,
        "Tab changed the well's height from \(wellFrame.height) to \(hiddenFrame.height)")
    try require(hiddenFrame.maxX == editor.view.bounds.maxX,
        "The hidden well ends at \(hiddenFrame.maxX), not \(editor.view.bounds.maxX)")
    try require(editor.validateUserInterfaceItem(panelsItem) && panelsItem.title == "Show Panels",
        "With everything hidden the panels item reads \(panelsItem.title)")
    try canvasKeysPress("Tab", code: 48, expect: "\t")
    try require(allShown(), "A second Tab did not show everything")
    editor.view.layoutSubtreeIfNeeded()
    try require(editor.scrollView.frame == wellFrame,
        "A second Tab left the well at \(editor.scrollView.frame), not \(wellFrame)")
    try require(editor.validateUserInterfaceItem(panelsItem) && panelsItem.title == "Hide Panels",
        "With everything shown the panels item reads \(panelsItem.title)")
    reported["tab_well"] = [Double(hiddenFrame.minX), Double(hiddenFrame.height)]

    try canvasKeysPress("Shift+Tab", code: 48, shift: true, expect: "\u{19}")
    try require(!editor.layersPanelVisible && editor.toolChromeVisible
                && !editor.optionsBar.isHidden && !editor.toolRail.isHidden,
        "Shift+Tab did more than hide the side panel")
    try canvasKeysPress("Shift+Tab", code: 48, shift: true, expect: "\u{19}")
    try require(allShown(), "A second Shift+Tab did not restore the side panel")

    try canvasKeysPress("Shift+Tab", code: 48, shift: true, expect: "\u{19}")
    try canvasKeysPress("Tab", code: 48, expect: "\t")
    try require(allHidden(), "Tab after Shift+Tab did not hide everything")
    try canvasKeysPress("Tab", code: 48, expect: "\t")
    try require(allShown(), "Tab did not show everything again")

    try require(panelsItem.keyEquivalent.isEmpty,
        "Hide Panels carries the key equivalent \(panelsItem.keyEquivalent)")
    try require(panelsItem.toolTip?.contains("Tab") == true,
        "Hide Panels' tooltip does not mention Tab")
    guard let panelsAction = panelsItem.action else {
        throw DoctoredCoreError(message: "Hide Panels has no action")
    }
    try require(NSApp.sendAction(panelsAction, to: nil, from: panelsItem),
        "Hide Panels found no target")
    try require(allHidden(), "View ▸ Hide Panels did not hide everything")
    try require(NSApp.sendAction(panelsAction, to: nil, from: panelsItem),
        "Show Panels found no target")
    try require(allShown(), "View ▸ Show Panels did not show everything")

    var entered = false
    var exited = false
    let center = NotificationCenter.default
    let enterToken = center.addObserver(forName: NSWindow.didEnterFullScreenNotification,
        object: window, queue: nil) { _ in entered = true }
    let exitToken = center.addObserver(forName: NSWindow.didExitFullScreenNotification,
        object: window, queue: nil) { _ in exited = true }
    defer {
        if window.styleMask.contains(.fullScreen) {
            exited = false
            window.toggleFullScreen(nil)
            let deadline = Date().addingTimeInterval(60)
            while !exited, Date() < deadline { pump() }
        }
        center.removeObserver(enterToken)
        center.removeObserver(exitToken)
    }
    try require(!window.styleMask.contains(.fullScreen), "The window starts in full screen")
    guard let fCode = canvasKeysCode(for: "f") else {
        throw DoctoredCoreError(message: "No key on the current layout types f")
    }
    reported["f_key_code"] = Int(fCode)
    try canvasKeysPress("F", code: fCode, expect: "f")
    try waitUntil({ entered }, "F never entered full screen")
    try require(window.styleMask.contains(.fullScreen), "F entered no full-screen style")
    try require(allShown(), "Entering full screen changed the chrome")
    try canvasKeysPress("F", code: fCode, expect: "f")
    try waitUntil({ exited }, "A second F never exited full screen")
    try require(!window.styleMask.contains(.fullScreen), "A second F left the full-screen style")
    try require(allShown(), "Exiting full screen changed the chrome")

    let fieldA = NSTextField(frame: CGRect(x: 0, y: 0, width: 120, height: 22))
    let fieldB = NSTextField(frame: CGRect(x: 0, y: 30, width: 120, height: 22))
    editor.view.addSubview(fieldA)
    editor.view.addSubview(fieldB)
    defer { fieldA.removeFromSuperview(); fieldB.removeFromSuperview() }
    fieldA.nextKeyView = fieldB
    fieldB.nextKeyView = fieldA
    try require(window.makeFirstResponder(fieldA), "Text field A refused first responder")
    try canvasKeysPress("Tab", code: 48, expect: "\t")
    try require(fieldB.currentEditor() != nil, "Tab in text field A did not move to B")
    try require(allShown(), "Tab in a text field changed the chrome")
    try canvasKeysPress("Shift+Tab", code: 48, shift: true, expect: "\u{19}")
    try require(fieldA.currentEditor() != nil, "Shift+Tab in text field B did not move to A")
    try require(allShown(), "Shift+Tab in a text field changed the chrome")
    try canvasKeysPress("F", code: fCode, expect: "f")
    pump(0.5)
    try require(!window.styleMask.contains(.fullScreen), "F in a text field entered full screen")
    try require(allShown(), "F in a text field changed the chrome")
    fieldA.removeFromSuperview()
    fieldB.removeFromSuperview()
    _ = window.makeFirstResponder(canvas)
    try require(document.history.entries.count == screenHistory
                && document.history.cursor == screenCursor,
        "The screen keys recorded history")
    reported["screen_keys"] = true

    return ["case": "canvas-keys-ui", "passed": true, "results": reported]
}
